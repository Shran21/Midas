//! In-session clipboard and file transfer. The browser speaks on its own `transfer` data channel;
//! the transport checks every message against the signed grant before the user worker, which does
//! the work with the signed-in user's own rights, sees it. Replies are checked the same way on
//! their way back, because neither the browser nor the worker is trusted.
//!
//! Control messages are JSON objects with a `type`, the same on the data channel and on the link
//! to the worker. File data travels as binary: `[kind][id: 16][offset: u64 LE][bytes]`, kind 1 to
//! the device and 2 from it.

use std::fmt;

use midas_remote_auth::Transfer as Rights;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// File bytes in one data message.
pub const CHUNK: usize = 16 * 1024;
/// Bytes a sender may have unacknowledged.
pub const WINDOW: u64 = 1024 * 1024;
/// Clipboard text, in UTF-8 bytes.
pub const CLIPBOARD_LIMIT: usize = 32 * 1024;
pub const FILE_LIMIT: u64 = 4 * 1024 * 1024 * 1024;
/// Characters of a file name.
pub const NAME_LIMIT: usize = 255;
/// UTF-8 bytes of a path.
pub const PATH_LIMIT: usize = 1024;
/// Entries of one directory listing; a larger directory is shown truncated.
pub const LISTING_LIMIT: usize = 1000;
/// Messages above this size are dropped unread; with its tag one still fits a link message.
pub const MESSAGE_LIMIT: usize = 60 * 1024;

const TO_DEVICE: u8 = 1;
const FROM_DEVICE: u8 = 2;
const DATA_HEADER: usize = 1 + 16 + 8;

/// Fixed-size bytes written as lowercase hexadecimal.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hex<const N: usize>(pub [u8; N]);
/// A transfer, chosen by the browser; resuming uses the same id.
pub type Id = Hex<16>;
pub type Sha256 = Hex<32>;

impl<const N: usize> fmt::Display for Hex<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}
impl<const N: usize> fmt::Debug for Hex<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl<const N: usize> Hex<N> {
    fn parse(text: &str) -> Option<Self> {
        if text.len() != 2 * N || !text.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
            return None;
        }
        let mut bytes = [0; N];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[2 * index..2 * index + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }
}
impl<const N: usize> Serialize for Hex<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
impl<'de, const N: usize> Deserialize<'de> for Hex<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <&str>::deserialize(deserializer)?;
        Self::parse(text).ok_or_else(|| de::Error::custom("hexadecimal value"))
    }
}

/// What the browser asks of the device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Puts text on the user's clipboard.
    ClipboardSet {
        text: String,
    },
    /// Reads the user's clipboard, only when the operator asks.
    ClipboardGet,
    /// Starts or resumes a file to the device; data follows from the offset in the answer.
    Upload {
        id: Id,
        name: String,
        size: u64,
    },
    /// All data was sent; the device checks the size and keeps the file.
    UploadEnd {
        id: Id,
    },
    /// A directory the user can open; `None` is the user's own profile folder.
    List {
        #[serde(default)]
        path: Option<String>,
    },
    /// Starts or resumes a file from the device. Size and modification time are from the listing,
    /// so a file that changed meanwhile is not mixed with the received part.
    Download {
        id: Id,
        path: String,
        size: u64,
        modified: i64,
        offset: u64,
    },
    /// The browser has stored a download up to this offset.
    Ack {
        id: Id,
        offset: u64,
    },
    Cancel {
        id: Id,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Problem {
    /// The clipboard or the file system could not be used now.
    Unavailable,
    /// The user's own rights do not allow it.
    Denied,
    NotFound,
    TooLarge,
    /// The file is not the one the listing showed.
    Changed,
    NoSpace,
    /// The clipboard holds no text.
    Empty,
    /// Out of order or unknown transfer.
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// Unix seconds.
    pub modified: i64,
}

/// What the device answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Clipboard {
        text: String,
    },
    ClipboardSet,
    /// Send data from `offset`; a resumed transfer starts above zero.
    UploadReady {
        id: Id,
        offset: u64,
    },
    /// Written up to `offset`.
    UploadAck {
        id: Id,
        offset: u64,
    },
    UploadDone {
        id: Id,
        /// The name the file got in the folder, which may differ on a collision.
        name: String,
        size: u64,
        sha256: Sha256,
    },
    Listing {
        path: String,
        #[serde(default)]
        parent: Option<String>,
        entries: Vec<Entry>,
        truncated: bool,
    },
    DownloadDone {
        id: Id,
        size: u64,
        sha256: Sha256,
    },
    Failed {
        #[serde(default)]
        id: Option<Id>,
        /// What failed when there is no transfer: `clipboard_set`, `clipboard_get` or `list`.
        #[serde(default)]
        operation: Option<Operation>,
        problem: Problem,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    ClipboardSet,
    ClipboardGet,
    List,
}

/// File bytes at an offset of one transfer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Data {
    pub id: Id,
    pub offset: u64,
    pub bytes: Vec<u8>,
}

/// One message in either direction on the channel or the link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message<T> {
    Control(T),
    Data(Data),
}

fn text_valid(text: &str, limit: usize) -> bool {
    text.len() <= limit && !text.contains('\0')
}

/// A file name as the user sees it: one component, no control characters.
pub fn name_valid(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= NAME_LIMIT
        && !name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
        && name != "."
        && name != ".."
}

fn path_valid(path: &str) -> bool {
    !path.is_empty() && path.len() <= PATH_LIMIT && !path.chars().any(char::is_control)
}

impl Request {
    /// Whether the grant allows it and its values are in range.
    pub fn allowed(&self, rights: Rights) -> bool {
        match self {
            Self::ClipboardSet { text } => {
                rights.clipboard && !text.is_empty() && text_valid(text, CLIPBOARD_LIMIT)
            }
            Self::ClipboardGet => rights.clipboard,
            Self::Upload { name, size, .. } => {
                rights.upload && name_valid(name) && *size <= FILE_LIMIT
            }
            Self::UploadEnd { .. } => rights.upload,
            Self::List { path } => rights.download && path.as_deref().is_none_or(path_valid),
            Self::Download {
                path, size, offset, ..
            } => rights.download && path_valid(path) && *size <= FILE_LIMIT && offset <= size,
            Self::Ack { offset, .. } => rights.download && *offset <= FILE_LIMIT,
            Self::Cancel { .. } => rights.upload || rights.download,
        }
    }
}

impl Reply {
    /// Whether the grant allows it and its values are in range; checked on the worker's answers.
    pub fn allowed(&self, rights: Rights) -> bool {
        let entries_valid = |entries: &[Entry]| {
            entries.len() <= LISTING_LIMIT
                && entries
                    .iter()
                    .all(|e| name_valid(&e.name) && e.size <= FILE_LIMIT)
        };
        match self {
            Self::Clipboard { text } => rights.clipboard && text_valid(text, CLIPBOARD_LIMIT),
            Self::ClipboardSet => rights.clipboard,
            Self::UploadReady { offset, .. } | Self::UploadAck { offset, .. } => {
                rights.upload && *offset <= FILE_LIMIT
            }
            Self::UploadDone { name, size, .. } => {
                rights.upload && name_valid(name) && *size <= FILE_LIMIT
            }
            Self::Listing {
                path,
                parent,
                entries,
                ..
            } => {
                rights.download
                    && path_valid(path)
                    && parent.as_deref().is_none_or(path_valid)
                    && entries_valid(entries)
            }
            Self::DownloadDone { size, .. } => rights.download && *size <= FILE_LIMIT,
            Self::Failed { id, operation, .. } => match (id, operation) {
                (Some(_), None) => rights.upload || rights.download,
                (None, Some(Operation::ClipboardSet | Operation::ClipboardGet)) => rights.clipboard,
                (None, Some(Operation::List)) => rights.download,
                _ => false,
            },
        }
    }
}

fn data_message(kind: u8, data: &Data) -> Vec<u8> {
    let mut message = Vec::with_capacity(DATA_HEADER + data.bytes.len());
    message.push(kind);
    message.extend_from_slice(&data.id.0);
    message.extend_from_slice(&data.offset.to_le_bytes());
    message.extend_from_slice(&data.bytes);
    message
}

fn read_data(kind: u8, message: &[u8]) -> Option<Data> {
    let (&[found], rest) = message.split_first_chunk::<1>()?;
    let (id, rest) = rest.split_first_chunk::<16>()?;
    let (offset, bytes) = rest.split_first_chunk::<8>()?;
    let offset = u64::from_le_bytes(*offset);
    (found == kind
        && (1..=CHUNK).contains(&bytes.len())
        && offset
            .checked_add(bytes.len() as u64)
            .is_some_and(|end| end <= FILE_LIMIT))
    .then(|| Data {
        id: Hex(*id),
        offset,
        bytes: bytes.to_vec(),
    })
}

fn read_control<T: for<'de> Deserialize<'de>>(message: &[u8]) -> Option<T> {
    (message.first() == Some(&b'{'))
        .then(|| serde_json::from_slice(message).ok())
        .flatten()
}

fn encode<T: Serialize>(message: &Message<T>, kind: u8) -> Vec<u8> {
    match message {
        Message::Control(control) => {
            serde_json::to_vec(control).expect("transfer messages serialize")
        }
        Message::Data(data) => data_message(kind, data),
    }
}

/// A browser message, or `None` when it is malformed or the grant does not allow it.
pub fn read_request(message: &[u8], rights: Rights) -> Option<Message<Request>> {
    if message.len() > MESSAGE_LIMIT {
        return None;
    }
    if let Some(data) = read_data(TO_DEVICE, message) {
        return rights.upload.then_some(Message::Data(data));
    }
    read_control::<Request>(message)
        .filter(|request| request.allowed(rights))
        .map(Message::Control)
}

/// The same message for the worker, or for the browser after the worker answered.
pub fn request_message(message: &Message<Request>) -> Vec<u8> {
    encode(message, TO_DEVICE)
}

/// A worker answer, or `None` when it is malformed or the grant does not allow it.
pub fn read_reply(message: &[u8], rights: Rights) -> Option<Message<Reply>> {
    if message.len() > MESSAGE_LIMIT {
        return None;
    }
    if let Some(data) = read_data(FROM_DEVICE, message) {
        return rights.download.then_some(Message::Data(data));
    }
    read_control::<Reply>(message)
        .filter(|reply| reply.allowed(rights))
        .map(Message::Control)
}

pub fn reply_message(message: &Message<Reply>) -> Vec<u8> {
    encode(message, FROM_DEVICE)
}

/// What kind of transfer an audit record is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    ClipboardToDevice,
    ClipboardFromDevice,
    Upload,
    Download,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Failed(Problem),
    Cancelled,
    /// The session ended first; the transfer may resume in another one.
    Interrupted,
}

/// One finished transfer for the audit: never its content, nor the clipboard's length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub kind: Kind,
    /// The file's name; for a file to the device, the name it got.
    pub name: Option<String>,
    pub size: Option<u64>,
    pub sha256: Option<Sha256>,
    pub outcome: Outcome,
}

/// Transfers open at once that the ledger follows; more are reported as interrupted.
const LEDGER_OPEN: usize = 64;
/// Reports waiting to be collected; beyond this they are dropped and counted.
const LEDGER_QUEUE: usize = 256;

/// Follows the checked messages of one session and turns them into audit reports.
#[derive(Default)]
pub struct Ledger {
    open: std::collections::HashMap<Id, (Kind, String, u64)>,
    reports: std::collections::VecDeque<Report>,
    dropped: u64,
}

fn file_part(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_owned()
}

impl Ledger {
    fn push(&mut self, report: Report) {
        if self.reports.len() < LEDGER_QUEUE {
            self.reports.push_back(report);
        } else {
            self.dropped += 1;
        }
    }

    fn file(&mut self, id: Id, kind: Kind, name: String, size: u64) {
        if self.open.len() >= LEDGER_OPEN && !self.open.contains_key(&id) {
            self.push(Report {
                kind,
                name: Some(name),
                size: Some(size),
                sha256: None,
                outcome: Outcome::Interrupted,
            });
            return;
        }
        self.open.insert(id, (kind, name, size));
    }

    fn finish(&mut self, id: Id, outcome: Outcome, sha256: Option<Sha256>, name: Option<String>) {
        if let Some((kind, opened, size)) = self.open.remove(&id) {
            self.push(Report {
                kind,
                name: Some(name.unwrap_or(opened)),
                size: Some(size),
                sha256,
                outcome,
            });
        }
    }

    pub fn request(&mut self, request: &Request) {
        match request {
            Request::Upload { id, name, size } => self.file(*id, Kind::Upload, name.clone(), *size),
            Request::Download { id, path, size, .. } => {
                self.file(*id, Kind::Download, file_part(path), *size);
            }
            Request::Cancel { id } => self.finish(*id, Outcome::Cancelled, None, None),
            _ => {}
        }
    }

    pub fn reply(&mut self, reply: &Reply) {
        let clipboard = |kind, outcome| Report {
            kind,
            name: None,
            size: None,
            sha256: None,
            outcome,
        };
        match reply {
            Reply::ClipboardSet => {
                self.push(clipboard(Kind::ClipboardToDevice, Outcome::Completed));
            }
            Reply::Clipboard { .. } => {
                self.push(clipboard(Kind::ClipboardFromDevice, Outcome::Completed));
            }
            Reply::UploadDone {
                id, name, sha256, ..
            } => self.finish(*id, Outcome::Completed, Some(*sha256), Some(name.clone())),
            Reply::DownloadDone { id, sha256, .. } => {
                self.finish(*id, Outcome::Completed, Some(*sha256), None);
            }
            Reply::Failed {
                id: Some(id),
                problem,
                ..
            } => self.finish(*id, Outcome::Failed(*problem), None, None),
            Reply::Failed {
                operation: Some(operation @ (Operation::ClipboardSet | Operation::ClipboardGet)),
                problem,
                ..
            } => {
                let kind = if *operation == Operation::ClipboardSet {
                    Kind::ClipboardToDevice
                } else {
                    Kind::ClipboardFromDevice
                };
                self.push(clipboard(kind, Outcome::Failed(*problem)));
            }
            _ => {}
        }
    }

    /// The session ended: every open transfer is reported as interrupted.
    pub fn close(&mut self) {
        let open: Vec<_> = self.open.keys().copied().collect();
        for id in open {
            self.finish(id, Outcome::Interrupted, None, None);
        }
    }

    pub fn take(&mut self) -> Option<Report> {
        self.reports.pop_front()
    }

    /// Reports that did not fit the queue.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests;
