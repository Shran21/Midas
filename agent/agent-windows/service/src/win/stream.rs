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
    link::{Command, Encoded, frame_messages},
};

use super::{capture::Duplicator, encode::SoftwareH264};
use crate::capture::{Frame, Selection};

struct Encoder {
    encoder: SoftwareH264,
    size: (u32, u32),
    fps: u32,
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
                    current.encoder.set_bitrate(bitrate(settings))?;
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
            self.encoder = Some(Encoder {
                encoder: SoftwareH264::new(size.0, size.1, fps, bitrate(self.settings))?,
                size,
                fps,
            });
            self.keyframe = true;
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
        let data = encoder.encoder.encode(&self.nv12, keyframe)?.to_vec();
        if data.is_empty() {
            self.keyframe |= keyframe;
            return Ok(None);
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
        Ok(frame_messages(&encoded))
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
    use midas_agent_core::media::link::Assembler;

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
            .find_map(|message| assembler.push(message).unwrap())
            .unwrap()
    }

    #[test]
    fn frames_follow_the_settings_and_keyframes_come_on_request() {
        let mut encoding = Encoding::new(Settings {
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
