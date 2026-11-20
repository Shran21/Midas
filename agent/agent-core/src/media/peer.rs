//! The agent side of the WebRTC connection. It answers the browser's offer, sends H.264 frames
//! over the negotiated payload type, carries data-channel messages both ways, and turns transport
//! statistics into samples for the adaptive controller. Media is DTLS-SRTP protected by str0m.

use std::{
    collections::VecDeque,
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
    sync::Once,
    time::{Duration, Instant},
};

use str0m::{
    Candidate, Event, IceConnectionState, Input, Output, Rtc, RtcConfig,
    bwe::{Bitrate, BweKind},
    change::SdpOffer,
    channel::ChannelId,
    format::Codec,
    media::{Direction, Frequency, MediaKind, MediaTime, Mid, Pt},
    net::{Protocol, Receive},
};

use super::{
    adapt::Sample,
    codec::{self, Choice, Encoder, Offer},
};

/// Datagrams larger than this are not WebRTC traffic.
const DATAGRAM: usize = 2048;
const STATS_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub enum PeerError {
    /// The offer is not valid SDP, or offers nothing this agent can answer.
    Offer,
    /// The offer has no video the agent may send.
    NoVideo,
    Socket(io::Error),
    Closed,
}

impl From<io::Error> for PeerError {
    fn from(error: io::Error) -> Self {
        Self::Socket(error)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PeerEvent {
    Connected,
    /// The browser lost a frame and needs a keyframe to recover.
    Keyframe,
    /// A data-channel message from the browser; untrusted input.
    Data(Vec<u8>),
    /// No usable H.264 format was negotiated; nothing can be sent.
    NoCodec,
    Closed,
}

pub(crate) fn crypto() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| str0m::crypto::from_feature_flags().install_process_default());
}

pub struct Answered {
    /// The SDP answer for the browser.
    pub answer: String,
    pub peer: Peer,
}

pub struct Peer {
    rtc: Rtc,
    socket: UdpSocket,
    local: SocketAddr,
    encoders: Vec<Encoder>,
    video: Option<(Mid, Pt)>,
    choice: Option<Choice>,
    channel: Option<ChannelId>,
    sample: Sample,
    started: Instant,
    events: VecDeque<PeerEvent>,
    buffer: Vec<u8>,
    closed: bool,
}

/// Answers `offer` on `socket`, advertising one host candidate per address in `hosts`.
pub fn answer(
    offer: &str,
    socket: UdpSocket,
    hosts: &[IpAddr],
    encoders: Vec<Encoder>,
    now: Instant,
) -> Result<Answered, PeerError> {
    crypto();
    // str0m reports media lines only once the connection is up, so check the offer itself.
    if !offer.lines().any(|line| line.starts_with("m=video ")) {
        return Err(if SdpOffer::from_sdp_string(offer).is_ok() {
            PeerError::NoVideo
        } else {
            PeerError::Offer
        });
    }
    let offer = SdpOffer::from_sdp_string(offer).map_err(|_| PeerError::Offer)?;
    let port = socket.local_addr()?.port();
    let local = socket.local_addr()?;
    let mut rtc = RtcConfig::new()
        .clear_codecs()
        .enable_h264(true)
        .enable_bwe(Some(Bitrate::kbps(1_000)))
        .set_stats_interval(Some(STATS_INTERVAL))
        .build(now);
    for host in hosts {
        let candidate =
            Candidate::host(SocketAddr::new(*host, port), "udp").map_err(|_| PeerError::Offer)?;
        rtc.add_local_candidate(candidate).ok_or(PeerError::Offer)?;
    }
    let answer = rtc
        .sdp_api()
        .accept_offer(offer)
        .map_err(|_| PeerError::Offer)?;
    socket.set_nonblocking(false)?;
    let mut peer = Peer {
        rtc,
        socket,
        local,
        encoders,
        video: None,
        choice: None,
        channel: None,
        sample: Sample::default(),
        started: now,
        events: VecDeque::new(),
        buffer: vec![0; DATAGRAM],
        closed: false,
    };
    peer.pump()?;
    Ok(Answered {
        answer: answer.to_sdp_string(),
        peer,
    })
}

impl Peer {
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// The negotiated format and the encoder that must produce it, known once connected.
    pub fn choice(&self) -> Option<&Choice> {
        self.choice.as_ref()
    }

    /// The limits of the encoder the negotiation chose.
    pub fn limits(&self) -> Option<super::adapt::Limits> {
        self.choice
            .as_ref()
            .map(|choice| self.encoders[choice.encoder].limits)
    }

    /// The latest transport measurements, taken out so each report counts once.
    pub fn take_sample(&mut self) -> Sample {
        std::mem::take(&mut self.sample)
    }

    pub fn next_event(&mut self) -> Option<PeerEvent> {
        self.events.pop_front()
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    fn choose(&mut self, mid: Mid) {
        let Some(writer) = self.rtc.writer(mid) else {
            return;
        };
        let offers: Vec<_> = writer
            .payload_params()
            .filter(|params| params.spec().codec == Codec::H264)
            .filter_map(|params| {
                let format = params.spec().format;
                Some(Offer {
                    payload_type: *params.pt(),
                    profile_level_id: format.profile_level_id?,
                    packetization_mode: format.packetization_mode.unwrap_or(0),
                })
            })
            .collect();
        self.choice = codec::choose(&offers, &self.encoders);
        match &self.choice {
            Some(choice) => self.video = Some((mid, Pt::from(choice.offer.payload_type))),
            None => {
                self.video = Some((mid, Pt::from(0)));
                self.events.push_back(PeerEvent::NoCodec);
            }
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Connected => {
                if let Some(limits) = self.limits() {
                    self.rtc.bwe().set_bitrate(
                        Bitrate::bps(limits.min_bitrate),
                        Bitrate::bps(limits.max_bitrate),
                    );
                }
                self.events.push_back(PeerEvent::Connected);
            }
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => self.close(),
            Event::MediaAdded(added)
                if added.kind == MediaKind::Video
                    && matches!(added.direction, Direction::SendOnly | Direction::SendRecv) =>
            {
                self.choose(added.mid)
            }
            Event::ChannelOpen(id, _) => self.channel = Some(id),
            Event::ChannelClose(id) if Some(id) == self.channel => self.close(),
            Event::ChannelData(data) => self.events.push_back(PeerEvent::Data(data.data)),
            Event::KeyframeRequest(_) => self.events.push_back(PeerEvent::Keyframe),
            Event::EgressBitrateEstimate(BweKind::Twcc { estimate, .. }) => {
                self.sample.estimate = Some(estimate.as_u64());
            }
            Event::PeerStats(stats) => {
                if let Some(loss) = stats.egress_loss_fraction {
                    self.sample.loss = Some(f64::from(loss));
                }
                self.sample.rtt = stats.rtt.or(self.sample.rtt);
            }
            Event::MediaEgressStats(stats) => {
                if let Some(loss) = stats.loss {
                    self.sample.loss = Some(f64::from(loss));
                }
                self.sample.rtt = stats.rtt.or(self.sample.rtt);
                if let Some(remote) = stats.remote {
                    self.sample.jitter =
                        Some(Duration::from_secs_f64(f64::from(remote.jitter) / 90_000.0));
                }
            }
            _ => {}
        }
    }

    /// Handles everything str0m wants to do now and returns when it next needs time.
    fn pump(&mut self) -> Result<Instant, PeerError> {
        loop {
            if !self.rtc.is_alive() {
                self.close();
                return Err(PeerError::Closed);
            }
            match self.rtc.poll_output() {
                Ok(Output::Timeout(at)) => return Ok(at),
                Ok(Output::Transmit(transmit)) => {
                    let _ = self
                        .socket
                        .send_to(&transmit.contents, transmit.destination);
                }
                Ok(Output::Event(event)) => self.event(event),
                Err(_) => {
                    self.close();
                    return Err(PeerError::Closed);
                }
            }
        }
    }

    /// Runs the connection until `deadline` or until it needs the caller (an event or a frame).
    pub fn drive(&mut self, deadline: Instant) -> Result<(), PeerError> {
        if self.closed {
            return Err(PeerError::Closed);
        }
        loop {
            let wake = self.pump()?.min(deadline);
            let wait = wake.saturating_duration_since(Instant::now());
            let input = if wait.is_zero() {
                Input::Timeout(Instant::now())
            } else {
                self.socket.set_read_timeout(Some(wait))?;
                match self.socket.recv_from(&mut self.buffer) {
                    Ok((length, source)) => match self.buffer[..length].try_into() {
                        Ok(contents) => Input::Receive(
                            Instant::now(),
                            Receive {
                                proto: Protocol::Udp,
                                source,
                                destination: self.local,
                                contents,
                            },
                        ),
                        Err(_) => continue,
                    },
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) =>
                    {
                        Input::Timeout(Instant::now())
                    }
                    // Windows reports an ICMP port-unreachable on the next read; keep going.
                    Err(error) if error.kind() == io::ErrorKind::ConnectionReset => continue,
                    Err(error) => return Err(error.into()),
                }
            };
            if self.rtc.handle_input(input).is_err() {
                self.close();
                return Err(PeerError::Closed);
            }
            if !self.events.is_empty() || Instant::now() >= deadline {
                return Ok(());
            }
        }
    }

    /// Sends one Annex B frame captured at `captured`. Frames before the connection is up, or
    /// without a negotiated format, are dropped, as a live picture never waits.
    pub fn send_frame(&mut self, frame: &[u8], captured: Instant) -> Result<bool, PeerError> {
        let (Some((mid, pt)), Some(_)) = (self.video, &self.choice) else {
            return Ok(false);
        };
        let Some(writer) = self.rtc.writer(mid) else {
            return Ok(false);
        };
        let micros = captured.saturating_duration_since(self.started).as_micros() as u64;
        let time = MediaTime::new(micros * 9 / 100, Frequency::NINETY_KHZ);
        let sent = writer.write(pt, captured, time, frame.to_vec()).is_ok();
        self.pump()?;
        Ok(sent)
    }

    /// Sends a data-channel message to the browser once the channel is open.
    pub fn send_data(&mut self, message: &[u8]) -> Result<bool, PeerError> {
        let Some(mut channel) = self.channel.and_then(|id| self.rtc.channel(id)) else {
            return Ok(false);
        };
        let sent = channel.write(false, message).unwrap_or(false);
        self.pump()?;
        Ok(sent)
    }

    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            self.rtc.disconnect();
            self.events.push_back(PeerEvent::Closed);
        }
    }
}

#[cfg(test)]
mod tests;
