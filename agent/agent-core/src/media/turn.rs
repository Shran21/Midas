//! A small TURN client (RFC 8656) for the agent's WebRTC side: one UDP allocation on a relay with
//! long-term credentials, permissions for the browser's addresses, and Send/Data indications. The
//! media stays DTLS-SRTP protected end to end, so the relay only carries encrypted packets.

use std::{
    collections::HashMap,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use sha1::Sha1;

const MAGIC: u32 = 0x2112_a442;
const HEADER: usize = 20;

const ALLOCATE: u16 = 0x0003;
const REFRESH: u16 = 0x0004;
const SEND: u16 = 0x0006;
const DATA: u16 = 0x0007;
const CREATE_PERMISSION: u16 = 0x0008;

const REQUEST: u16 = 0x0000;
const INDICATION: u16 = 0x0010;
const SUCCESS: u16 = 0x0100;
const FAILURE: u16 = 0x0110;

const USERNAME: u16 = 0x0006;
const MESSAGE_INTEGRITY: u16 = 0x0008;
const ERROR_CODE: u16 = 0x0009;
const LIFETIME: u16 = 0x000d;
const XOR_PEER_ADDRESS: u16 = 0x0012;
const DATA_ATTRIBUTE: u16 = 0x0013;
const REALM: u16 = 0x0014;
const NONCE: u16 = 0x0015;
const XOR_RELAYED_ADDRESS: u16 = 0x0016;
const REQUESTED_ADDRESS_FAMILY: u16 = 0x0017;
const REQUESTED_TRANSPORT: u16 = 0x0019;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;

const UDP: u8 = 17;
/// Permissions last five minutes on the relay; they are renewed well before.
const PERMISSION_RENEWAL: Duration = Duration::from_secs(240);
/// The allocation is refreshed this long before it would lapse.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);
const RETRANSMIT: Duration = Duration::from_millis(500);

/// A relay and the short-lived credentials the server issued for one session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub address: SocketAddr,
    pub username: String,
    pub password: String,
}

/// The first relay the agent can use over UDP (`turn:` without another transport), resolved.
pub fn udp_relay(relays: &[crate::remote::transport::Relay]) -> Option<Server> {
    use std::net::ToSocketAddrs;
    relays.iter().find_map(|relay| {
        relay.urls.iter().find_map(|url| {
            let rest = url.strip_prefix("turn:")?;
            let (endpoint, query) = rest.split_once('?').unwrap_or((rest, ""));
            if !query.is_empty() && query != "transport=udp" {
                return None;
            }
            let endpoint = if endpoint.rsplit_once(':').is_some_and(|(host, port)| {
                !host.is_empty()
                    && (host.ends_with(']') || !host.contains(':'))
                    && port.parse::<u16>().is_ok()
            }) {
                endpoint.to_owned()
            } else {
                format!("{endpoint}:3478")
            };
            let address = endpoint.to_socket_addrs().ok()?.next()?;
            Some(Server {
                address,
                username: relay.username.clone(),
                password: relay.credential.clone(),
            })
        })
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum TurnError {
    /// No answer from the relay in time.
    Timeout,
    /// The relay refused, with its STUN error code (401 bad credentials, 403 forbidden peer).
    Refused(u16),
    /// A malformed or unauthenticated answer.
    Invalid,
    Io(io::ErrorKind),
}

impl From<io::Error> for TurnError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

struct Message {
    kind: u16,
    transaction: [u8; 12],
    attributes: Vec<(u16, Vec<u8>)>,
}

impl Message {
    fn attribute(&self, kind: u16) -> Option<&[u8]> {
        self.attributes
            .iter()
            .find(|(attribute, _)| *attribute == kind)
            .map(|(_, value)| value.as_slice())
    }

    fn error(&self) -> Option<u16> {
        let value = self.attribute(ERROR_CODE)?;
        (value.len() >= 4).then(|| u16::from(value[2] & 0x07) * 100 + u16::from(value[3]))
    }
}

fn transaction() -> [u8; 12] {
    let mut id = [0; 12];
    id.copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..12]);
    id
}

fn padded(length: usize) -> usize {
    length.div_ceil(4) * 4
}

/// Encodes a message; with `key`, MESSAGE-INTEGRITY over everything before it is appended.
fn encode(
    kind: u16,
    transaction: &[u8; 12],
    attributes: &[(u16, &[u8])],
    key: Option<&[u8]>,
) -> Vec<u8> {
    let mut message = Vec::with_capacity(128);
    message.extend_from_slice(&kind.to_be_bytes());
    message.extend_from_slice(&[0, 0]);
    message.extend_from_slice(&MAGIC.to_be_bytes());
    message.extend_from_slice(transaction);
    for (attribute, value) in attributes {
        message.extend_from_slice(&attribute.to_be_bytes());
        message.extend_from_slice(&(value.len() as u16).to_be_bytes());
        message.extend_from_slice(value);
        message.resize(HEADER + padded(message.len() - HEADER), 0);
    }
    if let Some(key) = key {
        let length = (message.len() - HEADER + 24) as u16;
        message[2..4].copy_from_slice(&length.to_be_bytes());
        let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(key).expect("any key length");
        mac.update(&message);
        message.extend_from_slice(&MESSAGE_INTEGRITY.to_be_bytes());
        message.extend_from_slice(&20u16.to_be_bytes());
        message.extend_from_slice(&mac.finalize().into_bytes());
    } else {
        let length = (message.len() - HEADER) as u16;
        message[2..4].copy_from_slice(&length.to_be_bytes());
    }
    message
}

fn decode(bytes: &[u8]) -> Option<Message> {
    if bytes.len() < HEADER || bytes[0] & 0xc0 != 0 {
        return None;
    }
    let kind = u16::from_be_bytes([bytes[0], bytes[1]]);
    let length = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
    if u32::from_be_bytes(bytes[4..8].try_into().ok()?) != MAGIC
        || length % 4 != 0
        || bytes.len() != HEADER + length
    {
        return None;
    }
    let transaction = bytes[8..20].try_into().ok()?;
    let mut attributes = Vec::new();
    let mut rest = &bytes[HEADER..];
    while !rest.is_empty() {
        let (head, tail) = rest.split_at_checked(4)?;
        let attribute = u16::from_be_bytes([head[0], head[1]]);
        let size = usize::from(u16::from_be_bytes([head[2], head[3]]));
        let value = tail.get(..size)?;
        attributes.push((attribute, value.to_vec()));
        rest = tail.get(padded(size)..)?;
    }
    Some(Message {
        kind,
        transaction,
        attributes,
    })
}

/// Checks MESSAGE-INTEGRITY: the HMAC covers the message up to the attribute, with the length
/// field counting through it.
fn authentic(bytes: &[u8], key: &[u8]) -> bool {
    let mut offset = HEADER;
    while offset + 4 <= bytes.len() {
        let attribute = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        let size = usize::from(u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]));
        if attribute == MESSAGE_INTEGRITY {
            if size != 20 || offset + 24 > bytes.len() {
                return false;
            }
            let mut covered = bytes[..offset].to_vec();
            let length = (offset - HEADER + 24) as u16;
            covered[2..4].copy_from_slice(&length.to_be_bytes());
            let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(key).expect("any key length");
            mac.update(&covered);
            return mac.verify_slice(&bytes[offset + 4..offset + 24]).is_ok();
        }
        offset += 4 + padded(size);
    }
    false
}

fn xor_address(address: SocketAddr, transaction: &[u8; 12]) -> Vec<u8> {
    let port = address.port() ^ (MAGIC >> 16) as u16;
    let mut value = Vec::with_capacity(20);
    match address.ip() {
        IpAddr::V4(ip) => {
            value.extend_from_slice(&[0, 1]);
            value.extend_from_slice(&port.to_be_bytes());
            value.extend_from_slice(&(u32::from(ip) ^ MAGIC).to_be_bytes());
        }
        IpAddr::V6(ip) => {
            value.extend_from_slice(&[0, 2]);
            value.extend_from_slice(&port.to_be_bytes());
            let mut mask = MAGIC.to_be_bytes().to_vec();
            mask.extend_from_slice(transaction);
            value.extend(ip.octets().iter().zip(mask).map(|(byte, mask)| byte ^ mask));
        }
    }
    value
}

fn read_xor_address(value: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    let port = u16::from_be_bytes([*value.get(2)?, *value.get(3)?]) ^ (MAGIC >> 16) as u16;
    match (value.get(1)?, value.len()) {
        (1, 8) => {
            let ip = u32::from_be_bytes(value[4..8].try_into().ok()?) ^ MAGIC;
            Some(SocketAddr::new(Ipv4Addr::from(ip).into(), port))
        }
        (2, 20) => {
            let mut mask = MAGIC.to_be_bytes().to_vec();
            mask.extend_from_slice(transaction);
            let octets: [u8; 16] = value[4..20]
                .iter()
                .zip(mask)
                .map(|(byte, mask)| byte ^ mask)
                .collect::<Vec<_>>()
                .try_into()
                .ok()?;
            Some(SocketAddr::new(Ipv6Addr::from(octets).into(), port))
        }
        _ => None,
    }
}

fn long_term_key(username: &str, realm: &[u8], password: &str) -> [u8; 16] {
    let mut digest = Md5::new();
    digest.update(username.as_bytes());
    digest.update(b":");
    digest.update(realm);
    digest.update(b":");
    digest.update(password.as_bytes());
    digest.finalize().into()
}

#[derive(Clone, Copy, Debug)]
enum Pending {
    Permission(IpAddr),
    Refresh,
}

/// One relayed address on the relay, for one session.
pub struct Allocation {
    server: SocketAddr,
    username: String,
    realm: Vec<u8>,
    nonce: Vec<u8>,
    key: [u8; 16],
    pub relayed: SocketAddr,
    /// The agent's address as the relay saw it, a server-reflexive candidate.
    pub mapped: Option<SocketAddr>,
    expires: Instant,
    permissions: HashMap<IpAddr, Instant>,
    refused: Vec<IpAddr>,
    pending: HashMap<[u8; 12], (Pending, Vec<u8>, Instant)>,
}

/// What a packet from the relay was.
#[derive(Debug, PartialEq, Eq)]
pub enum Received {
    /// A packet the peer sent to the relayed address.
    Data(SocketAddr, Vec<u8>),
    /// An answer to one of our requests, already applied.
    Handled,
    /// Not from the relay, or not a valid TURN message for us.
    Ignored,
}

/// Allocates a relayed address of the same family as the relay's own address, waiting at most
/// until `deadline`. The socket must not be used by anything else meanwhile.
pub fn allocate(
    socket: &UdpSocket,
    server: &Server,
    deadline: Instant,
) -> Result<Allocation, TurnError> {
    let family: &[u8] = if server.address.is_ipv6() {
        &[2, 0, 0, 0]
    } else {
        &[1, 0, 0, 0]
    };
    let transport = [UDP, 0, 0, 0];
    let base = [
        (REQUESTED_TRANSPORT, &transport[..]),
        (REQUESTED_ADDRESS_FAMILY, family),
    ];
    let id = transaction();
    let challenge = exchange(
        socket,
        server.address,
        &encode(ALLOCATE | REQUEST, &id, &base, None),
        &id,
        deadline,
    )?;
    if challenge.error() != Some(401) {
        return Err(challenge
            .error()
            .map_or(TurnError::Invalid, TurnError::Refused));
    }
    let realm = challenge
        .attribute(REALM)
        .ok_or(TurnError::Invalid)?
        .to_vec();
    let mut nonce = challenge
        .attribute(NONCE)
        .ok_or(TurnError::Invalid)?
        .to_vec();
    let key = long_term_key(&server.username, &realm, &server.password);
    for _ in 0..2 {
        let id = transaction();
        let mut attributes = base.to_vec();
        attributes.extend([
            (USERNAME, server.username.as_bytes()),
            (REALM, realm.as_slice()),
            (NONCE, nonce.as_slice()),
        ]);
        let request = encode(ALLOCATE | REQUEST, &id, &attributes, Some(&key));
        let (answer, raw) = exchange_raw(socket, server.address, &request, &id, deadline)?;
        match (answer.kind, answer.error()) {
            (kind, _) if kind == ALLOCATE | SUCCESS => {
                if !authentic(&raw, &key) {
                    return Err(TurnError::Invalid);
                }
                let relayed = answer
                    .attribute(XOR_RELAYED_ADDRESS)
                    .and_then(|value| read_xor_address(value, &id))
                    .ok_or(TurnError::Invalid)?;
                let mapped = answer
                    .attribute(XOR_MAPPED_ADDRESS)
                    .and_then(|value| read_xor_address(value, &id));
                let lifetime = answer
                    .attribute(LIFETIME)
                    .and_then(|value| value.try_into().ok())
                    .map_or(600, u32::from_be_bytes);
                return Ok(Allocation {
                    server: server.address,
                    username: server.username.clone(),
                    realm,
                    nonce,
                    key,
                    relayed,
                    mapped,
                    expires: Instant::now() + Duration::from_secs(lifetime.into()),
                    permissions: HashMap::new(),
                    refused: Vec::new(),
                    pending: HashMap::new(),
                });
            }
            (_, Some(438)) => {
                nonce = answer.attribute(NONCE).ok_or(TurnError::Invalid)?.to_vec();
            }
            (_, Some(code)) => return Err(TurnError::Refused(code)),
            _ => return Err(TurnError::Invalid),
        }
    }
    Err(TurnError::Refused(438))
}

fn exchange(
    socket: &UdpSocket,
    server: SocketAddr,
    request: &[u8],
    id: &[u8; 12],
    deadline: Instant,
) -> Result<Message, TurnError> {
    exchange_raw(socket, server, request, id, deadline).map(|(message, _)| message)
}

/// Sends a request and waits for the matching answer from the relay, retransmitting meanwhile.
fn exchange_raw(
    socket: &UdpSocket,
    server: SocketAddr,
    request: &[u8],
    id: &[u8; 12],
    deadline: Instant,
) -> Result<(Message, Vec<u8>), TurnError> {
    let mut buffer = [0u8; 2048];
    let mut next = Instant::now();
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(TurnError::Timeout);
        }
        if now >= next {
            socket.send_to(request, server)?;
            next = now + RETRANSMIT;
        }
        socket.set_read_timeout(Some(
            next.min(deadline)
                .saturating_duration_since(now)
                .max(Duration::from_millis(1)),
        ))?;
        match socket.recv_from(&mut buffer) {
            Ok((length, source)) if source == server => {
                if let Some(message) = decode(&buffer[..length])
                    && &message.transaction == id
                {
                    return Ok((message, buffer[..length].to_vec()));
                }
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::ConnectionReset
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

impl Allocation {
    pub fn server(&self) -> SocketAddr {
        self.server
    }

    fn authenticated(
        &mut self,
        id: [u8; 12],
        method: u16,
        attributes: &[(u16, &[u8])],
        pending: Pending,
        now: Instant,
    ) -> Vec<u8> {
        let mut all = attributes.to_vec();
        all.extend([
            (USERNAME, self.username.as_bytes()),
            (REALM, self.realm.as_slice()),
            (NONCE, self.nonce.as_slice()),
        ]);
        let message = encode(method | REQUEST, &id, &all, Some(&self.key));
        self.pending.insert(id, (pending, message.clone(), now));
        message
    }

    /// Asks the relay to let `peer` reach the relayed address; the relay refuses forbidden ones.
    pub fn permit(&mut self, peer: IpAddr, now: Instant) -> Vec<u8> {
        self.permissions.insert(peer, now);
        // An IPv6 XOR address depends on the transaction id it travels in.
        let id = transaction();
        let value = xor_address(SocketAddr::new(peer, 0), &id);
        self.authenticated(
            id,
            CREATE_PERMISSION,
            &[(XOR_PEER_ADDRESS, &value)],
            Pending::Permission(peer),
            now,
        )
    }

    /// Wraps a packet sent from the relayed address; a permission request goes first when the
    /// peer has none or it is due for renewal. Refused peers get nothing.
    pub fn send(&mut self, destination: SocketAddr, payload: &[u8], now: Instant) -> Vec<Vec<u8>> {
        let peer = destination.ip();
        if self.refused.contains(&peer) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self
            .permissions
            .get(&peer)
            .is_none_or(|granted| now.saturating_duration_since(*granted) >= PERMISSION_RENEWAL)
        {
            out.push(self.permit(peer, now));
        }
        let id = transaction();
        let address = xor_address(destination, &id);
        out.push(encode(
            SEND | INDICATION,
            &id,
            &[(XOR_PEER_ADDRESS, &address), (DATA_ATTRIBUTE, payload)],
            None,
        ));
        out
    }

    /// Handles a packet from the relay.
    pub fn receive(
        &mut self,
        source: SocketAddr,
        packet: &[u8],
        now: Instant,
    ) -> (Received, Vec<Vec<u8>>) {
        if source != self.server {
            return (Received::Ignored, Vec::new());
        }
        let Some(message) = decode(packet) else {
            return (Received::Ignored, Vec::new());
        };
        if message.kind == DATA | INDICATION {
            return match (
                message
                    .attribute(XOR_PEER_ADDRESS)
                    .and_then(|value| read_xor_address(value, &message.transaction)),
                message.attribute(DATA_ATTRIBUTE),
            ) {
                (Some(peer), Some(data)) if self.permissions.contains_key(&peer.ip()) => {
                    (Received::Data(peer, data.to_vec()), Vec::new())
                }
                _ => (Received::Ignored, Vec::new()),
            };
        }
        let Some((pending, _, _)) = self.pending.remove(&message.transaction) else {
            return (Received::Ignored, Vec::new());
        };
        if message.kind & FAILURE == FAILURE {
            match message.error() {
                // A new nonce: the same request again with it.
                Some(438) => {
                    if let Some(nonce) = message.attribute(NONCE) {
                        self.nonce = nonce.to_vec();
                        let retry = match pending {
                            Pending::Permission(peer) => self.permit(peer, now),
                            Pending::Refresh => self.refresh(now),
                        };
                        return (Received::Handled, vec![retry]);
                    }
                }
                _ => {
                    if let Pending::Permission(peer) = pending {
                        self.permissions.remove(&peer);
                        self.refused.push(peer);
                    }
                }
            }
            return (Received::Handled, Vec::new());
        }
        if !authentic(packet, &self.key) {
            return (Received::Ignored, Vec::new());
        }
        if let Pending::Refresh = pending {
            let lifetime = message
                .attribute(LIFETIME)
                .and_then(|value| value.try_into().ok())
                .map_or(600, u32::from_be_bytes);
            self.expires = now + Duration::from_secs(lifetime.into());
        }
        (Received::Handled, Vec::new())
    }

    fn refresh(&mut self, now: Instant) -> Vec<u8> {
        let lifetime = 600u32.to_be_bytes();
        self.authenticated(
            transaction(),
            REFRESH,
            &[(LIFETIME, &lifetime)],
            Pending::Refresh,
            now,
        )
    }

    /// Requests due now: the allocation's refresh, and retransmissions of unanswered requests.
    pub fn due(&mut self, now: Instant) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = Vec::new();
        for (_, message, sent) in self.pending.values_mut() {
            if now.saturating_duration_since(*sent) >= RETRANSMIT * 2 {
                *sent = now;
                out.push(message.clone());
            }
        }
        let refreshing = self
            .pending
            .values()
            .any(|(pending, _, _)| matches!(pending, Pending::Refresh));
        if !refreshing && self.expires.saturating_duration_since(now) <= REFRESH_MARGIN {
            out.push(self.refresh(now));
        }
        out
    }

    /// When `due` next has something to send.
    pub fn next_due(&self) -> Instant {
        let retransmit = self
            .pending
            .values()
            .map(|(_, _, sent)| *sent + RETRANSMIT * 2)
            .min();
        let refresh = self
            .expires
            .checked_sub(REFRESH_MARGIN)
            .unwrap_or(self.expires);
        retransmit.map_or(refresh, |at| at.min(refresh))
    }

    /// Whether the relay refused `peer`.
    pub fn refused(&self, peer: IpAddr) -> bool {
        self.refused.contains(&peer)
    }

    /// Gives the relayed address back at the end of the session.
    pub fn release(&mut self, now: Instant) -> Vec<u8> {
        let lifetime = 0u32.to_be_bytes();
        self.authenticated(
            transaction(),
            REFRESH,
            &[(LIFETIME, &lifetime)],
            Pending::Refresh,
            now,
        )
    }
}

#[cfg(test)]
mod tests;
