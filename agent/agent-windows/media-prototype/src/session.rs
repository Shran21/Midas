use std::{
    fs::File,
    io::{BufWriter, ErrorKind, Write},
    net::UdpSocket,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Deserialize;
use serde_json::{Value, json};
use str0m::{
    Event, IceConnectionState, Input, Output, Rtc,
    bwe::{Bitrate, BweKind},
    channel::ChannelId,
    format::Codec,
    media::{Frequency, MediaKind, MediaTime, Mid},
    net::{Protocol, Receive},
};

use crate::{
    adapt::{Controller, MAX_BITRATE, MIN_BITRATE, Observation, Params, ladder},
    capture::Source,
    encode::H264,
    network::{Impairment, Profile, profile},
};

/// Called on the capture thread, so a source can attach that thread to a desktop.
pub type SourceFactory = Arc<dyn Fn() -> Box<dyn Source> + Send + Sync>;
pub type InputFactory = Arc<dyn Fn() -> Box<dyn InputSink> + Send + Sync>;

/// Receives remote pointer and keyboard events; dropping it releases anything still held.
pub trait InputSink: Send {
    /// `surface` is the desktop generation the viewer saw when it produced the event.
    fn input(&mut self, surface: u64, event: &Value);
}

#[derive(Clone)]
pub struct Hooks {
    pub source: SourceFactory,
    pub input: Option<InputFactory>,
}

pub struct Control {
    pub params: Params,
    pub keyframe: bool,
    pub stop: bool,
}

pub struct Encoded {
    pub data: Vec<u8>,
    pub captured: Instant,
    pub width: usize,
    pub height: usize,
    pub encode_ms: f64,
    pub source: &'static str,
    pub surface: (u64, &'static str),
}

/// Capture and encode on their own thread; the bounded channel drops frames instead of building latency.
pub fn spawn_encoder(
    control: Arc<Mutex<Control>>,
    frames: SyncSender<Encoded>,
    factory: SourceFactory,
) {
    thread::spawn(move || {
        let mut source = factory();
        let mut surface = None;
        let start = control.lock().map(|c| c.params).unwrap_or(Params {
            bitrate_bps: 2_500_000,
            scale: 1.0,
            fps: 30,
        });
        let Ok(mut encoder) = H264::new(start) else {
            return;
        };
        loop {
            let begun = Instant::now();
            let (params, keyframe) = {
                let Ok(mut c) = control.lock() else { return };
                if c.stop {
                    return;
                }
                (c.params, std::mem::take(&mut c.keyframe))
            };
            if encoder.reconfigure(params).is_err() {
                return;
            }
            if keyframe {
                encoder.keyframe();
            }
            let current = source.surface();
            if surface != Some(current) {
                surface = Some(current);
                encoder.keyframe();
            }
            if let Some(frame) = source.capture()
                && let Ok((data, (width, height))) = encoder.encode(frame)
                && !data.is_empty()
            {
                let encoded = Encoded {
                    data,
                    captured: begun,
                    width,
                    height,
                    encode_ms: begun.elapsed().as_secs_f64() * 1000.0,
                    source: source.name(),
                    surface: current,
                };
                match frames.try_send(encoded) {
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => return,
                }
            }
            let interval = Duration::from_secs_f64(1.0 / f64::from(params.fps.max(1)));
            if let Some(rest) = interval.checked_sub(begun.elapsed()) {
                thread::sleep(rest);
            }
        }
    });
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Command {
    Mode {
        auto: bool,
    },
    Params {
        bitrate_bps: u64,
        scale: f64,
        fps: u32,
    },
    Profile {
        name: String,
    },
    Trial,
    Client(Value),
    Input {
        surface: u64,
        event: Value,
    },
    Stop,
}

const TRIAL: [(&str, u64); 5] = [
    ("unlimited", 15),
    ("degraded", 20),
    ("bad", 20),
    ("degraded", 15),
    ("unlimited", 25),
];

struct Trial {
    started: Instant,
    phase: usize,
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

pub struct Session {
    rtc: Rtc,
    socket: UdpSocket,
    link: Impairment,
    controller: Controller,
    auto: bool,
    control: Arc<Mutex<Control>>,
    frames: Receiver<Encoded>,
    channel: Option<ChannelId>,
    media: Option<(Mid, str0m::format::PayloadParams)>,
    observed: Observation,
    log: BufWriter<File>,
    start: Instant,
    sent_bytes: u64,
    frames_sent: u64,
    last: Option<(usize, usize, f64, &'static str)>,
    trial: Option<Trial>,
    input: Option<Box<dyn InputSink>>,
    surface: Option<(u64, &'static str)>,
}

impl Session {
    pub fn new(
        rtc: Rtc,
        socket: UdpSocket,
        directory: PathBuf,
        hooks: Hooks,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!("webrtc-{}.jsonl", unix_ms()));
        tracing::info!(path = %path.display(), "measurement log");
        let start = Params {
            bitrate_bps: 2_500_000,
            scale: 1.0,
            fps: 30,
        };
        let control = Arc::new(Mutex::new(Control {
            params: start,
            keyframe: true,
            stop: false,
        }));
        let (sender, frames) = sync_channel(2);
        spawn_encoder(control.clone(), sender, hooks.source);
        let now = Instant::now();
        Ok(Self {
            rtc,
            socket,
            link: Impairment::new(
                profile("unlimited").unwrap_or(crate::network::PROFILES[0]),
                unix_ms() as u64,
            ),
            controller: Controller::new(start, now),
            auto: true,
            control,
            frames,
            channel: None,
            media: None,
            observed: Observation::default(),
            log: BufWriter::new(File::create(path)?),
            start: now,
            sent_bytes: 0,
            frames_sent: 0,
            last: None,
            trial: None,
            input: hooks.input.map(|factory| factory()),
            surface: None,
        })
    }

    fn record(&mut self, mut value: Value) {
        value["t_ms"] = json!(self.start.elapsed().as_millis() as u64);
        let _ = writeln!(self.log, "{value}");
        if let Some(id) = self.channel
            && value["type"] != "client"
            && let Some(mut channel) = self.rtc.channel(id)
        {
            let _ = channel.write(false, value.to_string().as_bytes());
        }
    }

    fn apply(&mut self, params: Params, reason: &str) {
        if let Ok(mut c) = self.control.lock() {
            c.params = params;
        }
        self.record(json!({ "type": "decision", "reason": reason, "params": params }));
    }

    fn set_profile(&mut self, next: Profile) {
        self.link.set_profile(next);
        self.record(json!({ "type": "profile", "profile": next }));
    }

    fn command(&mut self, data: &[u8]) {
        let Ok(command) = serde_json::from_slice::<Command>(data) else {
            return;
        };
        match command {
            Command::Mode { auto } => {
                self.auto = auto;
                self.record(json!({ "type": "mode", "auto": auto }));
            }
            Command::Params {
                bitrate_bps,
                scale,
                fps,
            } => {
                self.auto = false;
                let params = Params {
                    bitrate_bps,
                    scale,
                    fps,
                }
                .clamped();
                self.apply(params, "manual");
            }
            Command::Profile { name } => {
                if let Some(next) = profile(&name) {
                    self.set_profile(next);
                }
            }
            Command::Trial => {
                self.auto = true;
                self.trial = Some(Trial {
                    started: Instant::now(),
                    phase: 0,
                });
                if let Some(first) = profile(TRIAL[0].0) {
                    self.set_profile(first);
                }
                self.record(json!({ "type": "trial", "phase": 0, "profile": TRIAL[0].0 }));
            }
            Command::Client(stats) => self.record(json!({ "type": "client", "stats": stats })),
            // Input is never recorded: key events may carry typed credentials.
            Command::Input { surface, event } => {
                if let Some(sink) = &mut self.input {
                    sink.input(surface, &event);
                }
            }
            Command::Stop => {
                if let Ok(mut c) = self.control.lock() {
                    c.stop = true;
                }
                self.rtc.disconnect();
            }
        }
    }

    fn event(&mut self, event: Event) -> bool {
        match event {
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => return false,
            Event::Connected => {
                self.rtc
                    .bwe()
                    .set_bitrate(Bitrate::bps(MIN_BITRATE), Bitrate::bps(MAX_BITRATE));
                self.record(json!({ "type": "connected" }));
            }
            Event::MediaAdded(added) if added.kind == MediaKind::Video => {
                let params = self.rtc.writer(added.mid).and_then(|w| {
                    w.payload_params()
                        .find(|p| p.spec().codec == Codec::H264)
                        .cloned()
                });
                if let Some(params) = params {
                    self.media = Some((added.mid, params));
                }
            }
            Event::ChannelOpen(id, _) => self.channel = Some(id),
            // Closing the control channel ends the session; capture stops with it.
            Event::ChannelClose(id) if Some(id) == self.channel => return false,
            Event::ChannelData(data) => self.command(&data.data),
            Event::KeyframeRequest(_) => {
                if let Ok(mut c) = self.control.lock() {
                    c.keyframe = true;
                }
            }
            Event::EgressBitrateEstimate(BweKind::Twcc { estimate, .. }) => {
                self.observed.estimate_bps = Some(estimate.as_u64());
            }
            Event::MediaEgressStats(stats) => {
                self.observed.loss = stats.loss.map(f64::from);
                self.observed.rtt = stats.rtt;
            }
            _ => {}
        }
        true
    }

    fn tick(&mut self, now: Instant, elapsed: Duration) {
        if let Some(trial) = &mut self.trial {
            let mut boundary = 0;
            let mut index = TRIAL.len();
            for (i, (_, seconds)) in TRIAL.iter().enumerate() {
                boundary += seconds;
                if trial.started.elapsed() < Duration::from_secs(boundary) {
                    index = i;
                    break;
                }
            }
            if index != trial.phase {
                trial.phase = index;
                if index == TRIAL.len() {
                    self.trial = None;
                    self.record(json!({ "type": "trial_end" }));
                } else if let Some(next) = profile(TRIAL[index].0) {
                    self.set_profile(next);
                    self.record(
                        json!({ "type": "trial", "phase": index, "profile": TRIAL[index].0 }),
                    );
                }
            }
        }
        if self.auto
            && let Some((params, reason)) = self.controller.update(now, self.observed)
        {
            self.apply(params, reason);
        }
        let sent_bps = self.sent_bytes as f64 * 8.0 / elapsed.as_secs_f64();
        let fps = self.frames_sent as f64 / elapsed.as_secs_f64();
        self.sent_bytes = 0;
        self.frames_sent = 0;
        let target = self.control.lock().map(|c| c.params).ok();
        let (width, height, encode_ms, source) = self.last.unwrap_or((0, 0, 0.0, "none"));
        self.record(json!({
            "type": "stats",
            "profile": self.link.profile().name,
            "auto": self.auto,
            "estimate_bps": self.observed.estimate_bps,
            "loss": self.observed.loss,
            "rtt_ms": self.observed.rtt.map(|r| r.as_secs_f64() * 1000.0),
            "target": target,
            "sent_bps": sent_bps.round(),
            "sent_fps": (fps * 10.0).round() / 10.0,
            "width": width,
            "height": height,
            "encode_ms": (encode_ms * 10.0).round() / 10.0,
            "queue_ms": self.link.queue_delay(now).as_millis() as u64,
            "dropped_loss": self.link.dropped_loss,
            "dropped_queue": self.link.dropped_queue,
            "source": source,
        }));
        let _ = self.log.flush();
    }

    pub fn run(mut self) {
        let mut buffer = vec![0; 2000];
        let mut next_tick = Instant::now() + Duration::from_millis(500);
        let mut last_tick = Instant::now();
        loop {
            if !self.rtc.is_alive() {
                return self.finish();
            }
            let now = Instant::now();
            while let Ok(frame) = self.frames.try_recv() {
                if let Some((mid, params)) = &self.media
                    && let Some(writer) = self.rtc.writer(*mid)
                {
                    let micros = frame.captured.duration_since(self.start).as_micros() as u64;
                    let size = frame.data.len() as u64;
                    if writer
                        .write(
                            params.pt(),
                            frame.captured,
                            MediaTime::new(micros * 9 / 100, Frequency::NINETY_KHZ),
                            frame.data,
                        )
                        .is_ok()
                    {
                        self.sent_bytes += size;
                        self.frames_sent += 1;
                    }
                }
                self.last = Some((frame.width, frame.height, frame.encode_ms, frame.source));
                if self.surface != Some(frame.surface) {
                    self.surface = Some(frame.surface);
                    let (generation, desktop) = frame.surface;
                    self.record(
                        json!({ "type": "surface", "generation": generation, "desktop": desktop }),
                    );
                }
            }
            while let Some((destination, contents)) = self.link.pop_ready(now) {
                let _ = self.socket.send_to(&contents, destination);
            }
            let timeout = loop {
                match self.rtc.poll_output() {
                    Ok(Output::Timeout(t)) => break t,
                    Ok(Output::Transmit(t)) => {
                        self.link
                            .push(Instant::now(), t.destination, t.contents.to_vec())
                    }
                    Ok(Output::Event(e)) => {
                        if !self.event(e) {
                            return self.finish();
                        }
                    }
                    Err(_) => return self.finish(),
                }
            };
            if now >= next_tick {
                self.tick(now, now.duration_since(last_tick));
                last_tick = now;
                next_tick = now + Duration::from_millis(500);
            }
            let wake = [
                Some(timeout),
                self.link.next_release(),
                Some(next_tick),
                Some(now + Duration::from_millis(4)),
            ]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(now);
            let wait = wake.saturating_duration_since(Instant::now());
            if wait.is_zero() {
                if self
                    .rtc
                    .handle_input(Input::Timeout(Instant::now()))
                    .is_err()
                {
                    return self.finish();
                }
                continue;
            }
            if self.socket.set_read_timeout(Some(wait)).is_err() {
                return self.finish();
            }
            let input = match self.socket.recv_from(&mut buffer) {
                Ok((n, source)) => {
                    let Ok(destination) = self.socket.local_addr() else {
                        return self.finish();
                    };
                    let Ok(contents) = buffer[..n].try_into() else {
                        continue;
                    };
                    Input::Receive(
                        Instant::now(),
                        Receive {
                            proto: Protocol::Udp,
                            source,
                            destination,
                            contents,
                        },
                    )
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    Input::Timeout(Instant::now())
                }
                Err(_) => return self.finish(),
            };
            if self.rtc.handle_input(input).is_err() {
                return self.finish();
            }
        }
    }

    fn finish(mut self) {
        self.input = None;
        if let Ok(mut c) = self.control.lock() {
            c.stop = true;
        }
        self.record(json!({ "type": "closed" }));
        let _ = self.log.flush();
    }
}

/// The ladder exposed to the page for manual selection.
pub fn ladder_table() -> Value {
    json!(
        [2_500_000, 1_200_000, 600_000, 300_000]
            .iter()
            .map(|&b| {
                let (scale, fps) = ladder(b);
                json!({ "bitrate_bps": b, "scale": scale, "fps": fps })
            })
            .collect::<Vec<_>>()
    )
}
