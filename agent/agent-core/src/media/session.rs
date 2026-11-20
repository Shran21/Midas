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
use super::{
    adapt::{Controller, Limits, Profile, Settings, frame_size},
    link::{Assembler, Command, Desktop, Encoded, LinkError, read_desktop},
    peer::{Peer, PeerError, PeerEvent},
};

const ADAPT_EVERY: Duration = Duration::from_millis(500);
const STATS_EVERY: Duration = Duration::from_secs(1);
/// Input events queued for the worker; more than this means the worker is stuck.
const INPUT_QUEUE: usize = 256;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Control {
    Keyframe,
    Profile { profile: String },
}

/// Browser messages above this size are dropped unread.
const MESSAGE_LIMIT: usize = 4096;

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

    pub fn is_stopped(&self) -> bool {
        self.stopped
    }

    /// Ends the session: the worker stops capturing and the browser connection closes.
    pub fn stop(&mut self) {
        if !self.stopped {
            self.stopped = true;
            self.commands.push_back(Command::Stop);
            self.peer.close();
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
        if let Some(desktop) = read_desktop(message) {
            self.set_desktop(desktop);
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
                    if let Some(limits) = self.peer.limits() {
                        self.set_limits(limits, now);
                    }
                    self.commands.push_back(Command::Keyframe);
                }
                PeerEvent::Keyframe => self.commands.push_back(Command::Keyframe),
                PeerEvent::Data(data) => self.browser_message(&data, now),
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
