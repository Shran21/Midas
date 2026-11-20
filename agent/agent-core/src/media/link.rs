//! Messages between the capture worker and the protected transport. The worker sends encoded
//! frames, split to fit local control frames, and a keepalive while the desktop is unchanged; the
//! transport sends encoder settings, keyframe requests, input and the stop. Everything the worker
//! sends is untrusted and bounded here.

use serde::Deserialize;

use super::adapt::Settings;

/// Payload bytes per frame chunk; the local control channel carries at most 64 KiB per message.
pub const CHUNK: usize = 60 * 1024;
/// The largest encoded frame accepted, so a misbehaving worker cannot exhaust memory.
pub const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_CHUNKS: usize = MAX_FRAME.div_ceil(CHUNK);

const FRAME: u8 = 1;
const SETTINGS: u8 = 2;
const KEYFRAME: u8 = 3;
const STOP: u8 = 4;
const KEEPALIVE: u8 = 5;
const INPUT: u8 = 6;
const POINTER: u8 = 1;
const WHEEL: u8 = 2;
const KEY: u8 = 3;
/// Key codes are the browser's `KeyboardEvent.code` names, such as `KeyA` or `ArrowLeft`.
const CODE_LIMIT: usize = 32;

/// The worker sends this when there was nothing to send for a while, so the link stays open.
pub const KEEPALIVE_MESSAGE: [u8; 1] = [KEEPALIVE];

/// Pointer and keyboard input in video coordinates (0..=1), as the browser sends it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Input {
    Pointer { x: f64, y: f64, buttons: u8 },
    Wheel { dx: i16, dy: i16 },
    Key { code: String, down: bool },
}

impl Input {
    pub fn valid(&self) -> bool {
        match self {
            Self::Pointer { x, y, buttons } => {
                (0.0..=1.0).contains(x) && (0.0..=1.0).contains(y) && *buttons < 32
            }
            Self::Wheel { .. } => true,
            Self::Key { code, .. } => {
                !code.is_empty()
                    && code.len() <= CODE_LIMIT
                    && code.bytes().all(|b| b.is_ascii_alphanumeric())
            }
        }
    }
}

/// One encoded frame as the worker reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    pub sequence: u32,
    /// Capture time in microseconds since the stream started.
    pub captured: u64,
    pub width: u16,
    pub height: u16,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

/// Splits a frame into control messages: the first carries the header, each at most `CHUNK`
/// payload bytes. Frames above `MAX_FRAME` are not sent.
pub fn frame_messages(frame: &Encoded) -> Option<Vec<Vec<u8>>> {
    if frame.data.is_empty() || frame.data.len() > MAX_FRAME {
        return None;
    }
    let chunks: Vec<_> = frame.data.chunks(CHUNK).collect();
    let count = chunks.len() as u16;
    Some(
        chunks
            .into_iter()
            .enumerate()
            .map(|(index, chunk)| {
                let mut message = Vec::with_capacity(chunk.len() + 26);
                message.push(FRAME);
                message.extend_from_slice(&frame.sequence.to_le_bytes());
                message.extend_from_slice(&(index as u16).to_le_bytes());
                message.extend_from_slice(&count.to_le_bytes());
                if index == 0 {
                    message.extend_from_slice(&frame.captured.to_le_bytes());
                    message.extend_from_slice(&frame.width.to_le_bytes());
                    message.extend_from_slice(&frame.height.to_le_bytes());
                    message.push(u8::from(frame.keyframe));
                    message.extend_from_slice(&(frame.data.len() as u32).to_le_bytes());
                }
                message.extend_from_slice(chunk);
                message
            })
            .collect(),
    )
}

/// What the transport tells the worker.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Settings(Settings),
    Keyframe,
    Input(Input),
    Stop,
}

pub fn command_message(command: &Command) -> Vec<u8> {
    match command {
        Command::Settings(settings) => {
            let mut message = vec![SETTINGS];
            message.extend_from_slice(&settings.bitrate.to_le_bytes());
            message.extend_from_slice(&settings.scale.to_le_bytes());
            message.extend_from_slice(&settings.fps.to_le_bytes());
            message
        }
        Command::Keyframe => vec![KEYFRAME],
        Command::Stop => vec![STOP],
        Command::Input(Input::Pointer { x, y, buttons }) => {
            let scale =
                |value: f64| ((value.clamp(0.0, 1.0) * 65_535.0).round() as u16).to_le_bytes();
            let mut message = vec![INPUT, POINTER];
            message.extend_from_slice(&scale(*x));
            message.extend_from_slice(&scale(*y));
            message.push(*buttons);
            message
        }
        Command::Input(Input::Wheel { dx, dy }) => {
            let mut message = vec![INPUT, WHEEL];
            message.extend_from_slice(&dx.to_le_bytes());
            message.extend_from_slice(&dy.to_le_bytes());
            message
        }
        Command::Input(Input::Key { code, down }) => {
            let mut message = vec![INPUT, KEY, u8::from(*down)];
            message.extend_from_slice(code.as_bytes());
            message
        }
    }
}

pub fn read_command(message: &[u8]) -> Option<Command> {
    match message {
        [KEYFRAME] => Some(Command::Keyframe),
        [STOP] => Some(Command::Stop),
        [INPUT, POINTER, x0, x1, y0, y1, buttons] => {
            let unit = |low: u8, high: u8| f64::from(u16::from_le_bytes([low, high])) / 65_535.0;
            let input = Input::Pointer {
                x: unit(*x0, *x1),
                y: unit(*y0, *y1),
                buttons: *buttons,
            };
            input.valid().then_some(Command::Input(input))
        }
        [INPUT, WHEEL, x0, x1, y0, y1] => Some(Command::Input(Input::Wheel {
            dx: i16::from_le_bytes([*x0, *x1]),
            dy: i16::from_le_bytes([*y0, *y1]),
        })),
        [INPUT, KEY, down @ (0 | 1), code @ ..] => {
            let input = Input::Key {
                code: String::from_utf8(code.to_vec()).ok()?,
                down: *down == 1,
            };
            input.valid().then_some(Command::Input(input))
        }
        [SETTINGS, rest @ ..] if rest.len() == 20 => {
            let bitrate = u64::from_le_bytes(rest[..8].try_into().ok()?);
            let scale = f64::from_le_bytes(rest[8..16].try_into().ok()?);
            let fps = u32::from_le_bytes(rest[16..20].try_into().ok()?);
            (scale.is_finite() && scale > 0.0 && scale <= 1.0 && (1..=120).contains(&fps))
                .then_some(Command::Settings(Settings {
                    bitrate,
                    scale,
                    fps,
                }))
        }
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkError {
    /// A malformed or inconsistent message; the worker is misbehaving.
    Invalid,
}

struct Partial {
    header: Encoded,
    size: usize,
    count: u16,
    next: u16,
}

/// Rebuilds frames from chunks in order. A new sequence drops an unfinished frame: a late frame
/// is worth less than the next one.
#[derive(Default)]
pub struct Assembler {
    partial: Option<Partial>,
}

fn take<const N: usize>(bytes: &mut &[u8]) -> Option<[u8; N]> {
    let (head, rest) = bytes.split_first_chunk::<N>()?;
    *bytes = rest;
    Some(*head)
}

impl Assembler {
    pub fn push(&mut self, message: &[u8]) -> Result<Option<Encoded>, LinkError> {
        if message == KEEPALIVE_MESSAGE {
            return Ok(None);
        }
        let mut rest = message;
        let parse = |rest: &mut &[u8]| -> Option<(u32, u16, u16)> {
            let [kind] = take::<1>(rest)?;
            (kind == FRAME).then_some(())?;
            Some((
                u32::from_le_bytes(take(rest)?),
                u16::from_le_bytes(take(rest)?),
                u16::from_le_bytes(take(rest)?),
            ))
        };
        let (sequence, index, count) = parse(&mut rest).ok_or(LinkError::Invalid)?;
        if count == 0 || usize::from(count) > MAX_CHUNKS || index >= count {
            return Err(LinkError::Invalid);
        }
        if index == 0 {
            let header = (|| {
                let captured = u64::from_le_bytes(take(&mut rest)?);
                let width = u16::from_le_bytes(take(&mut rest)?);
                let height = u16::from_le_bytes(take(&mut rest)?);
                let [keyframe] = take::<1>(&mut rest)?;
                let size = u32::from_le_bytes(take(&mut rest)?) as usize;
                Some((captured, width, height, keyframe, size))
            })();
            let (captured, width, height, keyframe, size) = header.ok_or(LinkError::Invalid)?;
            if keyframe > 1
                || size == 0
                || size > MAX_FRAME
                || size.div_ceil(CHUNK) != usize::from(count)
                || width == 0
                || height == 0
            {
                return Err(LinkError::Invalid);
            }
            let mut data = Vec::with_capacity(size);
            data.extend_from_slice(rest);
            self.partial = Some(Partial {
                header: Encoded {
                    sequence,
                    captured,
                    width,
                    height,
                    keyframe: keyframe == 1,
                    data,
                },
                size,
                count,
                next: 1,
            });
        } else {
            let Some(partial) = self
                .partial
                .as_mut()
                .filter(|p| p.header.sequence == sequence && p.count == count && p.next == index)
            else {
                // A chunk of a frame we already dropped, or out of order: skip that frame.
                self.partial = None;
                return Ok(None);
            };
            partial.header.data.extend_from_slice(rest);
            partial.next += 1;
        }
        let partial = self.partial.as_ref().ok_or(LinkError::Invalid)?;
        let complete = partial.next == partial.count;
        let expected = if complete {
            partial.size
        } else {
            usize::from(partial.next) * CHUNK
        };
        if partial.header.data.len() != expected {
            self.partial = None;
            return Err(LinkError::Invalid);
        }
        Ok(complete
            .then(|| self.partial.take().map(|p| p.header))
            .flatten())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(size: usize, sequence: u32) -> Encoded {
        Encoded {
            sequence,
            captured: 123_456,
            width: 1920,
            height: 1080,
            keyframe: sequence == 0,
            data: (0..size).map(|i| (i % 251) as u8).collect(),
        }
    }

    #[test]
    fn frames_of_any_size_round_trip_in_bounded_messages() {
        for size in [1, CHUNK - 1, CHUNK, CHUNK + 1, 5 * CHUNK + 17] {
            let original = frame(size, 0);
            let messages = frame_messages(&original).unwrap();
            assert!(messages.iter().all(|m| m.len() <= CHUNK + 26));
            let mut assembler = Assembler::default();
            let mut result = None;
            for (index, message) in messages.iter().enumerate() {
                let done = assembler.push(message).unwrap();
                assert_eq!(done.is_some(), index + 1 == messages.len());
                result = done.or(result);
            }
            assert_eq!(result.unwrap(), original);
        }
        assert!(frame_messages(&frame(0, 1)).is_none());
        assert!(frame_messages(&frame(MAX_FRAME + 1, 1)).is_none());
    }

    #[test]
    fn a_new_frame_drops_the_unfinished_one_and_bad_input_is_refused() {
        let first = frame_messages(&frame(3 * CHUNK, 1)).unwrap();
        let second = frame_messages(&frame(10, 2)).unwrap();
        let mut assembler = Assembler::default();
        assert_eq!(assembler.push(&first[0]).unwrap(), None);
        assert_eq!(assembler.push(&second[0]).unwrap().unwrap().sequence, 2);
        assert_eq!(
            assembler.push(&first[1]).unwrap(),
            None,
            "stale chunk ignored"
        );
        assert_eq!(assembler.push(&first[2]).unwrap(), None);

        let mut assembler = Assembler::default();
        for bad in [
            vec![],
            vec![9, 0, 0],
            {
                let mut m = first[0].clone();
                m[7] = 0;
                m[8] = 0;
                m
            },
            {
                let mut m = second[0].clone();
                m.push(0);
                m
            },
            {
                let mut m = second[0].clone();
                m[21] = 2;
                m
            },
        ] {
            assert_eq!(assembler.push(&bad), Err(LinkError::Invalid), "{bad:?}");
        }
    }

    #[test]
    fn commands_round_trip_and_out_of_range_settings_are_refused() {
        let settings = Settings {
            bitrate: 2_000_000,
            scale: 0.75,
            fps: 30,
        };
        for command in [
            Command::Settings(settings),
            Command::Keyframe,
            Command::Stop,
            Command::Input(Input::Pointer {
                x: 0.0,
                y: 1.0,
                buttons: 3,
            }),
            Command::Input(Input::Wheel { dx: -120, dy: 5 }),
            Command::Input(Input::Key {
                code: "KeyA".into(),
                down: true,
            }),
        ] {
            assert_eq!(read_command(&command_message(&command)), Some(command));
        }
        let half = read_command(&command_message(&Command::Input(Input::Pointer {
            x: 0.5,
            y: 0.25,
            buttons: 0,
        })));
        let Some(Command::Input(Input::Pointer { x, y, .. })) = half else {
            panic!("pointer");
        };
        assert!((x - 0.5).abs() < 1e-4 && (y - 0.25).abs() < 1e-4);
        for (scale, fps) in [(0.0, 30), (1.5, 30), (f64::NAN, 30), (0.5, 0), (0.5, 500)] {
            let message = command_message(&Command::Settings(Settings {
                scale,
                fps,
                ..settings
            }));
            assert_eq!(read_command(&message), None);
        }
        for bad in [
            vec![INPUT, POINTER, 0, 0, 0, 0, 64],
            vec![INPUT, KEY, 2, b'A'],
            vec![INPUT, KEY, 1],
            vec![INPUT, KEY, 1, b' '],
            vec![INPUT, 9],
        ] {
            assert_eq!(read_command(&bad), None, "{bad:?}");
        }
        assert_eq!(Assembler::default().push(&KEEPALIVE_MESSAGE), Ok(None));
        assert_eq!(read_command(&[SETTINGS, 1, 2]), None);
        assert_eq!(read_command(&[FRAME]), None);
    }
}
