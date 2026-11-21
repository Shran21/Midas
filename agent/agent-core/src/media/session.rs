//! The transport side of one support session: it joins the browser connection, the adaptive
//! controller and the frames the capture worker sends. Browser messages are untrusted; only the
//! strictly parsed commands below pass, and input goes on to the worker for the lease check there.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use serde::Deserialize;
use serde_json::json;

pub use super::link::Input;
use super::transfer::{self, Ledger, Message, Report};
use super::{
    adapt::{Controller, Limits, Profile, Settings, frame_size},
    link::{
        Assembler, Command, Desktop, Encoded, EncoderReport, LinkError, read_desktop, read_encoder,
        read_sas_result, read_transfer_reply,
    },
    peer::{Peer, PeerError, PeerEvent},
};
use midas_remote_auth::Transfer as Rights;

const ADAPT_EVERY: Duration = Duration::from_millis(500);
const STATS_EVERY: Duration = Duration::from_secs(1);
/// Input events queued for the worker; more than this means the worker is stuck.
const INPUT_QUEUE: usize = 256;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Keyframe,
    Profile {
        profile: String,
    },
    /// Ctrl+Alt+Del: the transport side decides whether it may be sent.
    Sas,
}

/// Browser messages above this size are dropped unread.
const MESSAGE_LIMIT: usize = 4096;
/// Commands waiting for the worker; transfer messages beyond this are dropped, which breaks
/// off a transfer from a browser that ignores its window.
const COMMAND_QUEUE: usize = 1024;

pub struct Transport {
    peer: Peer,
    controller: Controller,
    limits: Limits,
    source: (u32, u32),
    assembler: Assembler,
    commands: VecDeque<Command>,
    inputs: VecDeque<Input>,
    sent: Option<Settings>,
    last_adapt: Instant,
    last_stats: Instant,
    frame: Option<(u16, u16)>,
    bytes: u64,
    frames: u32,
    stopped: bool,
    desktop: Desktop,
    /// What the grant allows besides picture and input.
    transfer: Rights,
    ledger: Ledger,
    /// The encoder the current source reports, which also sets the limits.
    encoder: Option<EncoderReport>,
}

impl Transport {
    /// `source` is the captured desktop size; the worker scales it as the settings say.
    pub fn new(peer: Peer, profile: Profile, source: (u32, u32), now: Instant) -> Self {
        let limits = peer.limits().unwrap_or(Limits::SOFTWARE);
        let controller = Controller::new(profile, limits, now);
        let mut transport = Self {
            peer,
            controller,
            limits,
            source,
            assembler: Assembler::default(),
            commands: VecDeque::new(),
            inputs: VecDeque::new(),
            sent: None,
            last_adapt: now,
            last_stats: now,
            frame: None,
            bytes: 0,
            frames: 0,
            stopped: false,
            desktop: Desktop::User,
            transfer: Rights::NONE,
            ledger: Ledger::default(),
            encoder: None,
        };
        transport.settings();
        transport
    }

    /// Uses the limits of the encoder the browser negotiation chose.
    pub fn set_limits(&mut self, limits: Limits, now: Instant) {
        self.limits = limits;
        let profile = self.controller.profile();
        self.controller = Controller::new(profile, limits, now);
        self.settings();
    }

    fn settings(&mut self) {
        let current = self.controller.current();
        if self.sent != Some(current) {
            self.sent = Some(current);
            self.commands.push_back(Command::Settings(current));
        }
    }

    /// The frame size the current settings produce for the captured desktop.
    pub fn frame_size(&self) -> (u32, u32) {
        frame_size(self.source, self.controller.current().scale, self.limits)
    }

    pub fn next_command(&mut self) -> Option<Command> {
        self.commands.pop_front()
    }

    pub fn next_input(&mut self) -> Option<Input> {
        self.inputs.pop_front()
    }

    /// Clipboard and file transfer as the grant allows; nothing without it.
    pub fn allow_transfer(&mut self, rights: Rights) {
        self.transfer = rights;
    }

    /// The next finished transfer for the audit.
    pub fn next_report(&mut self) -> Option<Report> {
        self.ledger.take()
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Ends the session: the worker stops capturing and the browser connection closes.
    pub fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.commands.push_back(Command::Stop);
            self.peer.close();
            self.ledger.close();
        }
    }

    /// The desktop the privileged component last reported.
    pub fn desktop(&self) -> Desktop {
        self.desktop
    }

    /// Frames come from the privileged component only while the secure desktop is shown. On a
    /// change of source the unfinished frame of the old one is dropped and a keyframe requested.
    fn set_desktop(&mut self, desktop: Desktop) {
        let switched = (self.desktop == Desktop::Secure) != (desktop == Desktop::Secure);
        self.desktop = desktop;
        if switched {
            self.assembler = Assembler::default();
            self.commands.push_back(Command::Keyframe);
        }
    }

    /// A message from the capture worker or a desktop report from the privileged component; a
    /// malformed one ends the session. The companion passes only the current source's frames.
    pub fn worker_message(&mut self, message: &[u8]) -> Result<(), LinkError> {
        if self.stopped {
            return Ok(());
        }
        if let Some(sent) = read_sas_result(message) {
            let answer = json!({
                "type": "sas",
                "result": if sent { "sent" } else { "unavailable" },
            });
            let _ = self.peer.send_data(answer.to_string().as_bytes());
            return Ok(());
        }
        if let Some(desktop) = read_desktop(message) {
            self.set_desktop(desktop);
            return Ok(());
        }
        if let Some(report) = read_encoder(message) {
            // A hardware encoder keeps up with more pixels and frames; the limits follow it.
            if self
                .encoder
                .as_ref()
                .is_none_or(|known| known.hardware != report.hardware)
            {
                let limits = if report.hardware {
                    Limits::HARDWARE
                } else {
                    Limits::SOFTWARE
                };
                self.set_limits(limits, Instant::now());
            }
            self.encoder = Some(report);
            return Ok(());
        }
        if let Some(reply) = read_transfer_reply(message) {
            let Some(reply) = transfer::read_reply(reply, self.transfer) else {
                self.stop();
                return Err(LinkError::Invalid);
            };
            if let Message::Control(control) = &reply {
                self.ledger.reply(control);
            }
            let binary = matches!(reply, Message::Data(_));
            let _ = self
                .peer
                .send_transfer(&transfer::reply_message(&reply), binary);
            return Ok(());
        }
        let frame = match self.assembler.push(message) {
            Ok(frame) => frame,
            Err(error) => {
                self.stop();
                return Err(error);
            }
        };
        if let Some(Encoded {
            data,
            width,
            height,
            ..
        }) = frame
            && self.peer.send_frame(&data, Instant::now()).unwrap_or(false)
        {
            self.frame = Some((width, height));
            self.bytes += data.len() as u64;
            self.frames += 1;
        }
        Ok(())
    }

    fn transfer_message(&mut self, data: &[u8]) {
        let Some(request) = transfer::read_request(data, self.transfer) else {
            return;
        };
        if self.commands.len() >= COMMAND_QUEUE {
            return;
        }
        if let Message::Control(control) = &request {
            self.ledger.request(control);
        }
        self.commands
            .push_back(Command::Transfer(transfer::request_message(&request)));
    }

    fn browser_message(&mut self, data: &[u8], now: Instant) {
        if data.len() > MESSAGE_LIMIT {
            return;
        }
        if let Ok(input) = serde_json::from_slice::<Input>(data) {
            if input.valid() && self.inputs.len() < INPUT_QUEUE {
                self.inputs.push_back(input);
            }
            return;
        }
        match serde_json::from_slice::<Control>(data) {
            Ok(Control::Keyframe) => self.commands.push_back(Command::Keyframe),
            Ok(Control::Sas) => self.commands.push_back(Command::Sas),
            Ok(Control::Profile { profile }) => {
                let profile = match profile.as_str() {
                    "text" => Profile::Text,
                    "motion" => Profile::Motion,
                    _ => return,
                };
                if self.controller.set_profile(profile, now).is_some() {
                    self.settings();
                }
            }
            Err(_) => {}
        }
    }

    fn report(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.last_stats).as_secs_f64();
        self.last_stats = now;
        let current = self.controller.current();
        let network = self.controller.network();
        let (width, height) = self.frame.unwrap_or((0, 0));
        let stats = json!({
            "type": "stats",
            "auto": true,
            "profile": match self.controller.profile() {
                Profile::Text => "text",
                Profile::Motion => "motion",
            },
            "width": width,
            "height": height,
            "target_bitrate": current.bitrate,
            "target_fps": current.fps,
            "sent_bitrate": (self.bytes as f64 * 8.0 / elapsed.max(0.001)).round(),
            "sent_fps": (f64::from(self.frames) / elapsed.max(0.001) * 10.0).round() / 10.0,
            "rtt_ms": network.rtt.map(|rtt| rtt.as_millis().min(60_000) as u64),
            "loss": network.loss.map(|loss| (loss * 1000.0).round() / 1000.0),
            "network": if network.degraded { "degraded" } else { "good" },
            "path": match self.peer.take_path() {
                Some(super::peer::Path::Relay) => Some("relay"),
                Some(super::peer::Path::Direct) => Some("direct"),
                None => None,
            },
            "desktop": match self.desktop {
                Desktop::User => "user",
                Desktop::Elevated => "elevated",
                Desktop::Secure => "secure",
            },
            "encoder": self.encoder.as_ref().map(|encoder| json!({
                "name": encoder.name,
                "hardware": encoder.hardware,
                "vendor": encoder.vendor,
                "latency_ms": encoder.latency_ms,
            })),
        });
        self.bytes = 0;
        self.frames = 0;
        let _ = self.peer.send_data(stats.to_string().as_bytes());
    }

    /// Runs the browser connection until `deadline`; adapts and reports on schedule.
    pub fn drive(&mut self, deadline: Instant) -> Result<(), PeerError> {
        if self.stopped {
            return Err(PeerError::Closed);
        }
        let result = self.peer.drive(deadline);
        let now = Instant::now();
        while let Some(event) = self.peer.next_event() {
            match event {
                PeerEvent::Connected => {
                    let reported = self
                        .encoder
                        .as_ref()
                        .filter(|encoder| encoder.hardware)
                        .map(|_| Limits::HARDWARE);
                    if let Some(limits) = reported.or_else(|| self.peer.limits()) {
                        self.set_limits(limits, now);
                    }
                    self.commands.push_back(Command::Keyframe);
                }
                PeerEvent::Keyframe => self.commands.push_back(Command::Keyframe),
                PeerEvent::Data(data) => self.browser_message(&data, now),
                PeerEvent::Transfer(data) => self.transfer_message(&data),
                PeerEvent::NoCodec | PeerEvent::Closed => self.stop(),
            }
        }
        if now.saturating_duration_since(self.last_adapt) >= ADAPT_EVERY {
            self.last_adapt = now;
            let mut sample = self.peer.take_sample();
            sample.queued_frames = 0;
            if self.controller.update(now, sample).is_some() {
                self.settings();
            }
        }
        if now.saturating_duration_since(self.last_stats) >= STATS_EVERY {
            self.report(now);
        }
        if result.is_err() {
            self.stop();
        }
        result
    }

    /// The worker could not keep up: the next adaptation lowers the frame rate first.
    pub fn backlog(&mut self, queued: usize, now: Instant) {
        let sample = super::adapt::Sample {
            queued_frames: queued,
            ..self.peer.take_sample()
        };
        if self.controller.update(now, sample).is_some() {
            self.settings();
        }
    }
}

#[cfg(test)]
mod tests;
