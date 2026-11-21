//! A browser stand-in for media tests: offers to receive video and opens a data channel.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

use str0m::{
    Candidate, Event, Input, Output, Rtc, RtcConfig,
    change::{SdpAnswer, SdpPendingOffer},
    channel::ChannelId,
    media::{Direction, MediaKind},
    net::{Protocol, Receive},
};

use super::{
    adapt::Limits,
    codec::{Encoder, Kind, Profile},
    peer::{Answered, Peer, PeerEvent, answer, crypto},
};

const DATAGRAM: usize = 2048;

/// A browser stand-in: offers to receive video and opens a data channel, over loopback UDP.
pub(crate) struct Browser {
    pub(crate) rtc: Rtc,
    socket: UdpSocket,
    local: SocketAddr,
    pub(crate) frames: Vec<Vec<u8>>,
    pub(crate) messages: Vec<Vec<u8>>,
    /// What arrived on the `transfer` channel.
    pub(crate) transfers: Vec<Vec<u8>>,
    channel: Option<ChannelId>,
    transfer: Option<ChannelId>,
    pub(crate) open: bool,
    buffer: Vec<u8>,
}

impl Browser {
    /// A browser whose only candidate is a host address on `ip`.
    pub(crate) fn offer_on(ip: IpAddr) -> (Self, String, SdpPendingOffer) {
        crypto();
        let socket = UdpSocket::bind((ip, 0)).unwrap();
        let local = socket.local_addr().unwrap();
        let mut rtc = RtcConfig::new()
            .clear_codecs()
            .enable_h264(true)
            .build(Instant::now());
        rtc.add_local_candidate(Candidate::host(local, "udp").unwrap())
            .unwrap();
        let mut change = rtc.sdp_api();
        change.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
        let channel = change.add_channel("control".into());
        let transfer = change.add_channel("transfer".into());
        let (offer, pending) = change.apply().unwrap();
        let browser = Self {
            rtc,
            socket,
            local,
            frames: Vec::new(),
            messages: Vec::new(),
            transfers: Vec::new(),
            channel: Some(channel),
            transfer: Some(transfer),
            open: false,
            buffer: vec![0; DATAGRAM],
        };
        (browser, offer.to_sdp_string(), pending)
    }

    fn pump(&mut self) -> Instant {
        loop {
            match self.rtc.poll_output().unwrap() {
                Output::Timeout(at) => return at,
                Output::Transmit(t) => {
                    let _ = self.socket.send_to(&t.contents, t.destination);
                }
                Output::Event(Event::MediaData(data)) => self.frames.push(data.data.to_vec()),
                Output::Event(Event::ChannelOpen(..)) => self.open = true,
                Output::Event(Event::ChannelData(data)) if Some(data.id) == self.transfer => {
                    self.transfers.push(data.data);
                }
                Output::Event(Event::ChannelData(data)) => self.messages.push(data.data),
                Output::Event(_) => {}
            }
        }
    }

    /// Handles every datagram that arrives within a few milliseconds, like a browser would.
    pub(crate) fn step(&mut self) {
        let until = Instant::now() + Duration::from_millis(5);
        loop {
            let wake = self.pump().min(until);
            let wait = wake.saturating_duration_since(Instant::now());
            if wait.is_zero() {
                self.rtc
                    .handle_input(Input::Timeout(Instant::now()))
                    .unwrap();
                if Instant::now() >= until {
                    self.pump();
                    return;
                }
                continue;
            }
            self.socket.set_read_timeout(Some(wait)).unwrap();
            let input = match self.socket.recv_from(&mut self.buffer) {
                Ok((length, source)) => Input::Receive(
                    Instant::now(),
                    Receive {
                        proto: Protocol::Udp,
                        source,
                        destination: self.local,
                        contents: self.buffer[..length].try_into().unwrap(),
                    },
                ),
                Err(_) => Input::Timeout(Instant::now()),
            };
            self.rtc.handle_input(input).unwrap();
        }
    }

    pub(crate) fn send_transfer(&mut self, message: &[u8]) -> bool {
        let Some(mut channel) = self.transfer.and_then(|id| self.rtc.channel(id)) else {
            return false;
        };
        channel.write(false, message).unwrap_or(false)
    }

    pub(crate) fn send(&mut self, message: &[u8]) -> bool {
        let Some(mut channel) = self.channel.and_then(|id| self.rtc.channel(id)) else {
            return false;
        };
        channel.write(false, message).unwrap_or(false)
    }
}

pub(crate) fn software() -> Vec<Encoder> {
    vec![Encoder {
        name: "software".into(),
        kind: Kind::Software,
        profiles: vec![Profile::ConstrainedBaseline, Profile::Baseline],
        limits: Limits::SOFTWARE,
    }]
}

/// A minimal Annex B keyframe: SPS, PPS and an IDR slice with filler payload.
pub(crate) fn keyframe(seed: u8) -> Vec<u8> {
    let mut frame = vec![0, 0, 0, 1, 0x67, 0x42, 0xe0, 0x1f, 0x8c, 0x8d, 0x40];
    frame.extend([0, 0, 0, 1, 0x68, 0xce, 0x3c, 0x80]);
    frame.extend([0, 0, 0, 1, 0x65, 0x88, 0x84]);
    frame.extend((0..3000u32).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) | 1));
    frame
}

pub(crate) fn connect(encoders: Vec<Encoder>) -> (Browser, Peer, Vec<PeerEvent>) {
    let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let socket = UdpSocket::bind((localhost, 0)).unwrap();
    connect_with(encoders, localhost, socket, &[localhost], None)
}

/// Connects a browser on `browser` to an agent answering on `socket` with `hosts` and `relay`.
pub(crate) fn connect_with(
    encoders: Vec<Encoder>,
    browser: IpAddr,
    socket: UdpSocket,
    hosts: &[IpAddr],
    relay: Option<super::turn::Allocation>,
) -> (Browser, Peer, Vec<PeerEvent>) {
    let (mut browser, offer, pending) = Browser::offer_on(browser);
    let Answered { answer, mut peer } =
        answer(&offer, socket, hosts, relay, encoders, Instant::now()).unwrap();
    browser
        .rtc
        .sdp_api()
        .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events = Vec::new();
    while !(events.contains(&PeerEvent::Connected) && browser.open && peer.choice().is_some()
        || events.contains(&PeerEvent::NoCodec))
    {
        assert!(Instant::now() < deadline, "no connection: {events:?}");
        browser.step();
        peer.drive(Instant::now() + Duration::from_millis(5))
            .unwrap();
        while let Some(event) = peer.next_event() {
            events.push(event);
        }
    }
    (browser, peer, events)
}
