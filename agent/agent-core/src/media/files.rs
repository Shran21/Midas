//! The user worker's side of in-session transfer. It runs with the signed-in user's own rights,
//! so the operating system decides what may be read or written. Files to the device go into one
//! folder (`Downloads\MIDAS` on Windows) as a partial file named after the transfer, so a later
//! session resumes it; only a complete file gets its own name.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256 as Hasher};

use super::transfer::{
    CHUNK, CLIPBOARD_LIMIT, Data, Entry, Hex, Id, LISTING_LIMIT, MESSAGE_LIMIT, Message, Operation,
    Problem, Reply, Request, WINDOW, name_valid, reply_message,
};

/// Acknowledge a file to the device after this many new bytes.
const ACK_STEP: u64 = 64 * 1024;
/// Rough bytes one listing may take on the wire, below the message limit.
const LISTING_BYTES: usize = 48 * 1024;
/// Partial files nobody resumed for this long are removed.
const ABANDONED: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const PARTIAL: &str = ".midas-part";

/// What differs between operating systems.
pub trait Platform {
    /// The clipboard's text; `None` when it holds none.
    fn clipboard(&mut self) -> Result<Option<String>, Problem>;
    fn set_clipboard(&mut self, text: &str) -> Result<(), Problem>;
    /// Bytes the user may still write on the volume of `path`.
    fn free_space(&self, path: &Path) -> Option<u64>;
}

struct Upload {
    file: File,
    name: String,
    size: u64,
    written: u64,
    acked: u64,
    hasher: Hasher,
    partial: PathBuf,
}

struct Download {
    file: File,
    size: u64,
    sent: u64,
    acked: u64,
    hasher: Hasher,
}

pub struct Files<P> {
    platform: P,
    home: PathBuf,
    folder: PathBuf,
    uploads: HashMap<Id, Upload>,
    downloads: HashMap<Id, Download>,
}

fn problem(error: &io::Error) -> Problem {
    match error.kind() {
        io::ErrorKind::NotFound => Problem::NotFound,
        io::ErrorKind::PermissionDenied => Problem::Denied,
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Problem::NoSpace,
        _ => Problem::Unavailable,
    }
}

fn seconds(time: io::Result<SystemTime>) -> i64 {
    time.ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_secs() as i64)
}

fn failed(id: Id, problem: Problem) -> Vec<Message<Reply>> {
    vec![Message::Control(Reply::Failed {
        id: Some(id),
        operation: None,
        problem,
    })]
}

fn operation_failed(operation: Operation, problem: Problem) -> Vec<Message<Reply>> {
    vec![Message::Control(Reply::Failed {
        id: None,
        operation: Some(operation),
        problem,
    })]
}

/// Adds the first `length` bytes of `file` to `hasher`, leaving the file positioned after them.
fn hash_prefix(file: &mut File, length: u64, hasher: &mut Hasher) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    let mut buffer = vec![0; 256 * 1024];
    let mut left = length;
    while left > 0 {
        let wanted = left.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..wanted])?;
        hasher.update(&buffer[..wanted]);
        left -= wanted as u64;
    }
    Ok(())
}

/// A path the user named, only on a local drive: no network share (which would send the user's
/// credentials to whoever answers), no device namespace, no alternate data stream, no `..`.
pub fn local_path(text: &str) -> Option<PathBuf> {
    local_path_for(text, cfg!(windows))
}

fn local_path_for(text: &str, windows: bool) -> Option<PathBuf> {
    if windows {
        let bytes = text.as_bytes();
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\';
        let rest = text.get(3..)?;
        let components_valid = rest
            .split('\\')
            .filter(|part| !part.is_empty())
            .all(|part| part != "." && part != ".." && !part.contains([':', '/']));
        (drive && components_valid).then(|| PathBuf::from(text))
    } else {
        let components_valid = text
            .split('/')
            .filter(|part| !part.is_empty())
            .all(|part| part != "." && part != "..");
        (text.starts_with('/') && components_valid).then(|| PathBuf::from(text))
    }
}

fn reserved(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit())
}

/// A name every file system takes; the user's characters are kept where they can be.
fn file_name(name: &str) -> String {
    let mut clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    while clean.ends_with(['.', ' ']) {
        clean.pop();
    }
    if clean.is_empty() {
        clean = "file".into();
    }
    let stem = clean.split('.').next().unwrap_or_default();
    if reserved(stem.trim_end()) {
        clean.insert(0, '_');
    }
    // Never mistaken for a partial file.
    if clean.to_ascii_lowercase().ends_with(PARTIAL) {
        clean.push('_');
    }
    clean
}

/// `name`, or `name (2).ext` and so on when the folder already has it.
fn unused(folder: &Path, name: &str) -> Option<PathBuf> {
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    std::iter::once(name.to_owned())
        .chain((2..1000).map(|n| format!("{stem} ({n}){extension}")))
        .map(|candidate| folder.join(candidate))
        .find(|path| !path.exists())
}

impl<P: Platform> Files<P> {
    /// `home` is listed when the browser names no folder; files to the device go into `folder`.
    pub fn new(platform: P, home: PathBuf, folder: PathBuf) -> Self {
        let files = Self {
            platform,
            home,
            folder,
            uploads: HashMap::new(),
            downloads: HashMap::new(),
        };
        files.sweep();
        files
    }

    fn sweep(&self) {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return;
        };
        for entry in entries.flatten() {
            let abandoned = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > ABANDONED);
            let partial = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with('.') && name.ends_with(PARTIAL));
            if partial && abandoned {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    fn partial(&self, id: Id) -> PathBuf {
        self.folder.join(format!(".{id}{PARTIAL}"))
    }

    pub fn handle(&mut self, message: Message<Request>) -> Vec<Message<Reply>> {
        match message {
            Message::Data(data) => self.data(data),
            Message::Control(request) => match request {
                Request::ClipboardSet { text } => match self.platform.set_clipboard(&text) {
                    Ok(()) => vec![Message::Control(Reply::ClipboardSet)],
                    Err(problem) => operation_failed(Operation::ClipboardSet, problem),
                },
                Request::ClipboardGet => match self.platform.clipboard() {
                    Ok(Some(text)) if text.len() > CLIPBOARD_LIMIT => {
                        operation_failed(Operation::ClipboardGet, Problem::TooLarge)
                    }
                    Ok(Some(text)) if !text.is_empty() => {
                        let reply = Message::Control(Reply::Clipboard { text });
                        // Escaped control characters can make short text a long message.
                        if reply_message(&reply).len() > MESSAGE_LIMIT {
                            operation_failed(Operation::ClipboardGet, Problem::TooLarge)
                        } else {
                            vec![reply]
                        }
                    }
                    Ok(_) => operation_failed(Operation::ClipboardGet, Problem::Empty),
                    Err(problem) => operation_failed(Operation::ClipboardGet, problem),
                },
                Request::Upload { id, name, size } => self.upload(id, name, size),
                Request::UploadEnd { id } => self.finish(id),
                Request::List { path } => self.list(path.as_deref()),
                Request::Download {
                    id,
                    path,
                    size,
                    modified,
                    offset,
                } => self.download(id, &path, size, modified, offset),
                Request::Ack { id, offset } => self.ack(id, offset),
                Request::Cancel { id } => {
                    if self.uploads.remove(&id).is_some() {
                        let _ = fs::remove_file(self.partial(id));
                    }
                    self.downloads.remove(&id);
                    Vec::new()
                }
            },
        }
    }

    fn upload(&mut self, id: Id, name: String, size: u64) -> Vec<Message<Reply>> {
        self.uploads.remove(&id);
        let partial = self.partial(id);
        let opened = (|| {
            fs::create_dir_all(&self.folder)?;
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&partial)?;
            let mut written = file.metadata()?.len();
            if written > size {
                file.set_len(0)?;
                written = 0;
            }
            let mut hasher = Hasher::new();
            hash_prefix(&mut file, written, &mut hasher)?;
            Ok::<_, io::Error>((file, written, hasher))
        })();
        let (file, written, hasher) = match opened {
            Ok(opened) => opened,
            Err(error) => return failed(id, problem(&error)),
        };
        if self
            .platform
            .free_space(&self.folder)
            .is_some_and(|free| free < size - written)
        {
            if written == 0 {
                drop(file);
                let _ = fs::remove_file(&partial);
            }
            return failed(id, Problem::NoSpace);
        }
        self.uploads.insert(
            id,
            Upload {
                file,
                name,
                size,
                written,
                acked: written,
                hasher,
                partial,
            },
        );
        vec![Message::Control(Reply::UploadReady {
            id,
            offset: written,
        })]
    }

    fn data(&mut self, data: Data) -> Vec<Message<Reply>> {
        let Some(upload) = self.uploads.get_mut(&data.id) else {
            return failed(data.id, Problem::Invalid);
        };
        let end = data.offset + data.bytes.len() as u64;
        if data.offset != upload.written || end > upload.size {
            self.uploads.remove(&data.id);
            return failed(data.id, Problem::Invalid);
        }
        if let Err(error) = upload.file.write_all(&data.bytes) {
            self.uploads.remove(&data.id);
            return failed(data.id, problem(&error));
        }
        upload.hasher.update(&data.bytes);
        upload.written = end;
        if end - upload.acked >= ACK_STEP || end == upload.size {
            upload.acked = end;
            return vec![Message::Control(Reply::UploadAck {
                id: data.id,
                offset: end,
            })];
        }
        Vec::new()
    }

    fn finish(&mut self, id: Id) -> Vec<Message<Reply>> {
        let Some(upload) = self.uploads.remove(&id) else {
            return failed(id, Problem::Invalid);
        };
        if upload.written != upload.size {
            return failed(id, Problem::Invalid);
        }
        if let Err(error) = upload.file.sync_all() {
            return failed(id, problem(&error));
        }
        drop(upload.file);
        let Some(target) = unused(&self.folder, &file_name(&upload.name)) else {
            return failed(id, Problem::Unavailable);
        };
        if let Err(error) = fs::rename(&upload.partial, &target) {
            return failed(id, problem(&error));
        }
        let name = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        vec![Message::Control(Reply::UploadDone {
            id,
            name,
            size: upload.size,
            sha256: Hex(upload.hasher.finalize().into()),
        })]
    }

    fn list(&mut self, path: Option<&str>) -> Vec<Message<Reply>> {
        let folder = match path {
            None => self.home.clone(),
            Some(text) => match local_path(text) {
                Some(path) => path,
                None => return operation_failed(Operation::List, Problem::Invalid),
            },
        };
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => return operation_failed(Operation::List, problem(&error)),
        };
        let mut listed: Vec<Entry> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let metadata = entry.metadata().ok()?;
                let dir = metadata.is_dir();
                name_valid(&name).then(|| Entry {
                    name,
                    dir,
                    size: if dir { 0 } else { metadata.len() },
                    modified: seconds(metadata.modified()),
                })
            })
            .collect();
        listed.sort_by(|a, b| {
            b.dir
                .cmp(&a.dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let mut budget = LISTING_BYTES;
        let mut kept = 0;
        for entry in &listed {
            let cost = entry.name.len() * 2 + 64;
            if kept == LISTING_LIMIT || cost > budget {
                break;
            }
            budget -= cost;
            kept += 1;
        }
        let truncated = kept < listed.len();
        listed.truncate(kept);
        let text = |path: &Path| path.to_str().map(str::to_owned);
        let Some(shown) = text(&folder) else {
            return operation_failed(Operation::List, Problem::Invalid);
        };
        let parent = folder.parent().and_then(text);
        vec![Message::Control(Reply::Listing {
            path: shown,
            parent,
            entries: listed,
            truncated,
        })]
    }

    fn download(
        &mut self,
        id: Id,
        path: &str,
        size: u64,
        modified: i64,
        offset: u64,
    ) -> Vec<Message<Reply>> {
        self.downloads.remove(&id);
        let Some(path) = local_path(path) else {
            return failed(id, Problem::Invalid);
        };
        let opened = (|| {
            let mut file = File::open(&path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            if metadata.len() != size || seconds(metadata.modified()) != modified {
                return Ok(None);
            }
            let mut hasher = Hasher::new();
            hash_prefix(&mut file, offset, &mut hasher)?;
            Ok(Some((file, hasher)))
        })();
        let (file, hasher) = match opened {
            Ok(Some(opened)) => opened,
            Ok(None) => return failed(id, Problem::Changed),
            Err(error) => return failed(id, problem(&error)),
        };
        self.downloads.insert(
            id,
            Download {
                file,
                size,
                sent: offset,
                acked: offset,
                hasher,
            },
        );
        self.pump(id)
    }

    fn ack(&mut self, id: Id, offset: u64) -> Vec<Message<Reply>> {
        let Some(download) = self.downloads.get_mut(&id) else {
            return Vec::new();
        };
        if offset < download.acked || offset > download.sent {
            self.downloads.remove(&id);
            return failed(id, Problem::Invalid);
        }
        download.acked = offset;
        self.pump(id)
    }

    /// Sends what the window allows; the last chunk is followed by the file's digest.
    fn pump(&mut self, id: Id) -> Vec<Message<Reply>> {
        let mut out = Vec::new();
        let Some(download) = self.downloads.get_mut(&id) else {
            return out;
        };
        while download.sent < download.size && download.sent - download.acked < WINDOW {
            let length = (download.size - download.sent).min(CHUNK as u64) as usize;
            let mut bytes = vec![0; length];
            if let Err(error) = download.file.read_exact(&mut bytes) {
                self.downloads.remove(&id);
                out.extend(failed(id, problem(&error)));
                return out;
            }
            download.hasher.update(&bytes);
            out.push(Message::Data(Data {
                id,
                offset: download.sent,
                bytes,
            }));
            download.sent += length as u64;
        }
        if download.sent == download.size
            && let Some(done) = self.downloads.remove(&id)
        {
            out.push(Message::Control(Reply::DownloadDone {
                id,
                size: done.size,
                sha256: Hex(done.hasher.finalize().into()),
            }));
        }
        out
    }
}

#[cfg(test)]
mod tests;
