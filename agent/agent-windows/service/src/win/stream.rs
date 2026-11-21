//! The capture worker's video stream: it captures the selected monitors, converts and encodes
//! them at the transport's settings, and splits each frame into link messages. A desktop change
//! or a request starts with a keyframe; an unchanged desktop sends nothing.

use std::{
    io,
    time::{Duration, Instant},
};

use midas_agent_core::media::{
    adapt::{Limits, Settings, frame_size},
    frame::{Bgra, bgra_to_nv12},
    link::{Command, Encoded, EncoderReport, encoder_message, frame_messages},
};

use super::{
    capture::Duplicator,
    encode::{
        SoftwareH264,
        hardware::{HardwareH264, candidates},
    },
};
use crate::capture::{Frame, Selection};

/// How often the encoder in use and its latency are reported.
const REPORT_EVERY: Duration = Duration::from_secs(1);

enum Backend {
    Software(SoftwareH264),
    Hardware(HardwareH264),
}

impl Backend {
    fn encode(&mut self, nv12: &[u8], keyframe: bool) -> io::Result<Vec<u8>> {
        match self {
            Self::Software(encoder) => encoder.encode(nv12, keyframe).map(<[u8]>::to_vec),
            Self::Hardware(encoder) => encoder.encode(nv12, keyframe).map(<[u8]>::to_vec),
        }
    }

    fn set_bitrate(&mut self, bitrate: u32) -> io::Result<()> {
        match self {
            Self::Software(encoder) => encoder.set_bitrate(bitrate),
            Self::Hardware(encoder) => encoder.set_bitrate(bitrate),
        }
    }

    fn report(&self, software: Option<Duration>) -> EncoderReport {
        let millis = |latency: Duration| latency.as_millis().min(u128::from(u16::MAX - 1)) as u16;
        match self {
            Self::Software(_) => EncoderReport {
                hardware: false,
                vendor: String::new(),
                name: "Media Foundation H.264".into(),
                latency_ms: software.map(millis),
            },
            Self::Hardware(encoder) => EncoderReport {
                hardware: true,
                vendor: encoder.vendor.as_str().into(),
                name: encoder.name.clone(),
                latency_ms: encoder.latency().map(millis),
            },
        }
    }
}

struct Encoder {
    backend: Backend,
    size: (u32, u32),
    fps: u32,
}

/// The first hardware encoder that starts, if hardware may still be tried.
fn hardware(size: (u32, u32), fps: u32, bitrate: u32) -> Option<HardwareH264> {
    let found = candidates()
        .inspect_err(|error| tracing::debug!(%error, "no hardware encoders listed"))
        .ok()?;
    found.iter().find_map(|candidate| {
        HardwareH264::new(candidate, size.0, size.1, fps, bitrate)
            .inspect_err(|error| {
                tracing::info!(encoder = %candidate.name, %error, "hardware encoder did not start");
            })
            .ok()
    })
}

/// Encoding at the transport's settings; the encoder is rebuilt only for a new size or rate.
pub struct Encoding {
    limits: Limits,
    settings: Settings,
    encoder: Option<Encoder>,
    keyframe: bool,
    nv12: Vec<u8>,
    sequence: u32,
    started: Instant,
    /// Hardware is tried until it fails once; the software encoder serves the rest.
    try_hardware: bool,
    reported: Option<Instant>,
    software_latency: Option<Duration>,
}

fn bitrate(settings: Settings) -> u32 {
    settings.bitrate.min(u64::from(u32::MAX)) as u32
}

impl Encoding {
    pub fn new(settings: Settings) -> Self {
        Self {
            limits: Limits::SOFTWARE,
            settings,
            encoder: None,
            keyframe: true,
            nv12: Vec::new(),
            sequence: 0,
            started: Instant::now(),
            try_hardware: true,
            reported: None,
            software_latency: None,
        }
    }

    /// Software only, as after a hardware failure.
    #[cfg(test)]
    fn software_only(settings: Settings) -> Self {
        Self {
            try_hardware: false,
            ..Self::new(settings)
        }
    }

    /// Applies a transport command; returns false on stop.
    pub fn command(&mut self, command: Command) -> io::Result<bool> {
        match command {
            Command::Stop => return Ok(false),
            Command::Keyframe => self.keyframe = true,
            // Input is executed beside the stream, not by the encoder.
            Command::Input(_) | Command::Release | Command::Sas | Command::Transfer(_) => {}
            Command::Settings(settings) => {
                let changed = settings.bitrate != self.settings.bitrate;
                self.settings = settings;
                if changed && let Some(current) = &mut self.encoder {
                    current.backend.set_bitrate(bitrate(settings))?;
                }
            }
        }
        Ok(true)
    }

    pub fn interval(&self) -> Duration {
        Duration::from_secs(1) / self.settings.fps.max(1)
    }

    pub fn request_keyframe(&mut self) {
        self.keyframe = true;
    }

    pub fn keyframe_pending(&self) -> bool {
        self.keyframe
    }

    /// Encodes `frame` and returns its link messages, or `None` while the encoder buffers.
    pub fn encode(&mut self, frame: &Frame, captured: Instant) -> io::Result<Option<Vec<Vec<u8>>>> {
        let size = frame_size(
            (frame.width, frame.height),
            self.settings.scale,
            self.limits,
        );
        let fps = self.settings.fps;
        if self
            .encoder
            .as_ref()
            .is_none_or(|current| current.size != size || current.fps != fps)
        {
            // The old encoder goes first: a hardware one may hold the device the next needs.
            self.encoder = None;
            let rate = bitrate(self.settings);
            let backend = match self
                .try_hardware
                .then(|| hardware(size, fps, rate))
                .flatten()
            {
                Some(encoder) => Backend::Hardware(encoder),
                None => Backend::Software(SoftwareH264::new(size.0, size.1, fps, rate)?),
            };
            self.limits = match backend {
                Backend::Hardware(_) => Limits::HARDWARE,
                Backend::Software(_) => Limits::SOFTWARE,
            };
            self.encoder = Some(Encoder { backend, size, fps });
            self.keyframe = true;
            self.reported = None;
        }
        bgra_to_nv12(
            Bgra {
                data: &frame.bgra,
                width: frame.width,
                height: frame.height,
                stride: frame.width as usize * 4,
            },
            size.0,
            size.1,
            &mut self.nv12,
        )
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let keyframe = std::mem::take(&mut self.keyframe);
        let encoder = self.encoder.as_mut().expect("encoder just set");
        let started = Instant::now();
        let data = match encoder.backend.encode(&self.nv12, keyframe) {
            Ok(data) => data,
            // A failing hardware encoder is replaced by the software one with the next frame.
            Err(error) if matches!(encoder.backend, Backend::Hardware(_)) => {
                tracing::warn!(%error, "hardware encoder failed; the software encoder takes over");
                self.try_hardware = false;
                self.encoder = None;
                self.limits = Limits::SOFTWARE;
                self.keyframe = true;
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if matches!(encoder.backend, Backend::Software(_)) {
            self.software_latency = Some(started.elapsed());
        }
        let report = self
            .reported
            .is_none_or(|at| at.elapsed() >= REPORT_EVERY)
            .then(|| encoder_message(&encoder.backend.report(self.software_latency)));
        if report.is_some() {
            self.reported = Some(Instant::now());
        }
        if data.is_empty() {
            self.keyframe |= keyframe;
            return Ok(report.map(|report| vec![report]));
        }
        let encoded = Encoded {
            sequence: self.sequence,
            captured: captured.saturating_duration_since(self.started).as_micros() as u64,
            width: size.0 as u16,
            height: size.1 as u16,
            keyframe,
            data,
        };
        self.sequence = self.sequence.wrapping_add(1);
        Ok(frame_messages(&encoded).map(|mut messages| {
            messages.extend(report);
            messages
        }))
    }
}

pub struct Stream {
    duplicator: Duplicator,
    encoding: Encoding,
    /// The last captured desktop, re-encoded when the browser asks for a keyframe.
    last: Option<Frame>,
    generation: u64,
}

impl Stream {
    pub fn open(selection: &Selection, settings: Settings) -> io::Result<Self> {
        let duplicator = Duplicator::open(selection)?;
        let generation = duplicator.generation();
        Ok(Self {
            duplicator,
            encoding: Encoding::new(settings),
            last: None,
            generation,
        })
    }

    /// The captured area of the virtual desktop, for mapping input onto it.
    pub fn view(&self) -> crate::capture::Rect {
        self.duplicator.view()
    }

    /// The captured view's size, before scaling.
    pub fn source(&self) -> (u32, u32) {
        let view = self.duplicator.view();
        (view.width, view.height)
    }

    pub fn command(&mut self, command: Command) -> io::Result<bool> {
        self.encoding.command(command)
    }

    /// Waits up to one frame interval for a desktop change and returns the encoded frame's link
    /// messages, or `None` when nothing needs sending.
    pub fn next_frame(&mut self) -> io::Result<Option<Vec<Vec<u8>>>> {
        let captured = Instant::now();
        let image = self.duplicator.capture(self.encoding.interval())?.cloned();
        if self.duplicator.generation() != self.generation {
            self.generation = self.duplicator.generation();
            self.encoding.request_keyframe();
        }
        if image.is_some() && self.duplicator.fresh() {
            self.last = image;
        } else if !self.encoding.keyframe_pending() {
            return Ok(None);
        }
        match &self.last {
            Some(frame) => {
                let frame = frame.clone();
                self.encoding.encode(&frame, captured)
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use midas_agent_core::media::link::{Assembler, is_frame, read_encoder};

    use super::*;

    fn desktop(shift: u8) -> Frame {
        let mut frame = Frame::blank(1280, 720);
        for (index, pixel) in frame.bgra.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let value = ((index % 1280) as u8).wrapping_add(shift);
            pixel.copy_from_slice(&[value, value / 2, 255 - value, 255]);
        }
        frame
    }

    fn assemble(messages: &[Vec<u8>]) -> Encoded {
        let mut assembler = Assembler::default();
        messages
            .iter()
            .filter(|message| is_frame(message))
            .find_map(|message| assembler.push(message).unwrap())
            .unwrap()
    }

    fn reports(messages: &[Vec<u8>]) -> Vec<EncoderReport> {
        messages.iter().filter_map(|m| read_encoder(m)).collect()
    }

    #[test]
    fn the_encoder_in_use_is_reported_with_its_latency() {
        let settings = Settings {
            bitrate: 1_000_000,
            scale: 0.5,
            fps: 15,
        };
        let mut software = Encoding::software_only(settings);
        let first = match software.encode(&desktop(0), Instant::now()) {
            Ok(messages) => messages.unwrap(),
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                println!("skipped: {error}");
                return;
            }
            Err(error) => panic!("encoding: {error}"),
        };
        let report = &reports(&first)[0];
        assert!(!report.hardware && report.vendor.is_empty());
        assert!(report.latency_ms.is_some());
        // Within the second nothing new is reported.
        let next = software
            .encode(&desktop(1), Instant::now())
            .unwrap()
            .unwrap();
        assert!(reports(&next).is_empty());
        // Where a hardware encoder exists it is used first and reported as such.
        let mut encoding = Encoding::new(settings);
        let mut seen = Vec::new();
        for shift in 0..10 {
            if let Some(messages) = encoding.encode(&desktop(shift), Instant::now()).unwrap() {
                seen.extend(reports(&messages));
            }
        }
        let report = seen.first().expect("a report");
        println!("encoder in use: {report:?}");
        if report.hardware {
            assert!(!report.vendor.is_empty());
            assert_eq!(encoding.limits, Limits::HARDWARE);
        }
    }

    #[test]
    fn frames_follow_the_settings_and_keyframes_come_on_request() {
        let mut encoding = Encoding::software_only(Settings {
            bitrate: 1_000_000,
            scale: 0.5,
            fps: 15,
        });
        let first = match encoding.encode(&desktop(0), Instant::now()) {
            Ok(messages) => assemble(&messages.unwrap()),
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                println!("skipped: {error}");
                return;
            }
            Err(error) => panic!("encoding: {error}"),
        };
        assert!(first.keyframe);
        assert_eq!((first.width, first.height), (640, 360));
        let next = assemble(
            &encoding
                .encode(&desktop(3), Instant::now())
                .unwrap()
                .unwrap(),
        );
        assert!(!next.keyframe && next.sequence == 1);
        assert!(encoding.command(Command::Keyframe).unwrap());
        let key = assemble(
            &encoding
                .encode(&desktop(5), Instant::now())
                .unwrap()
                .unwrap(),
        );
        assert!(key.keyframe);
        let larger = Settings {
            bitrate: 2_000_000,
            scale: 1.0,
            fps: 30,
        };
        assert!(encoding.command(Command::Settings(larger)).unwrap());
        let resized = assemble(
            &encoding
                .encode(&desktop(7), Instant::now())
                .unwrap()
                .unwrap(),
        );
        assert!(resized.keyframe, "a new size starts with a keyframe");
        assert_eq!((resized.width, resized.height), (1280, 720));
        assert_eq!(encoding.interval(), Duration::from_secs(1) / 30);
        assert!(!encoding.command(Command::Stop).unwrap());
    }

    #[test]
    fn the_real_desktop_streams_where_it_can_be_duplicated() {
        let settings = Settings {
            bitrate: 1_000_000,
            scale: 0.5,
            fps: 15,
        };
        let mut stream = match Stream::open(&Selection::Combined, settings) {
            Ok(stream) => stream,
            Err(error) => {
                println!("skipped: no desktop to capture: {error}");
                return;
            }
        };
        match (0..30).find_map(|_| stream.next_frame().transpose()) {
            Some(Ok(messages)) => {
                let frame = assemble(&messages);
                assert!(frame.keyframe);
                let (width, height) = frame_size(stream.source(), 0.5, Limits::SOFTWARE);
                assert_eq!(
                    (u32::from(frame.width), u32::from(frame.height)),
                    (width, height)
                );
            }
            Some(Err(error)) if error.kind() == io::ErrorKind::Unsupported => {
                println!("skipped: {error}");
            }
            Some(Err(error)) => panic!("stream: {error}"),
            None => println!("skipped: the desktop cannot be duplicated in this session"),
        }
    }
}
