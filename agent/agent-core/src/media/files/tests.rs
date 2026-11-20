use std::sync::atomic::{AtomicU32, Ordering};

use super::{super::transfer::Sha256, *};

#[derive(Default)]
struct Fake {
    clipboard: Option<String>,
    free: Option<u64>,
}

impl Platform for Fake {
    fn clipboard(&mut self) -> Result<Option<String>, Problem> {
        Ok(self.clipboard.clone())
    }
    fn set_clipboard(&mut self, text: &str) -> Result<(), Problem> {
        self.clipboard = Some(text.to_owned());
        Ok(())
    }
    fn free_space(&self, _: &Path) -> Option<u64> {
        self.free
    }
}

/// A fresh folder under the system's temporary directory, removed when dropped.
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "midas-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn files(scratch: &Scratch, fake: Fake) -> Files<Fake> {
    Files::new(fake, scratch.0.clone(), scratch.0.join("MIDAS"))
}

fn id(byte: u8) -> Id {
    Hex([byte; 16])
}

fn digest(bytes: &[u8]) -> Sha256 {
    Hex(Hasher::digest(bytes).into())
}

fn control(replies: Vec<Message<Reply>>) -> Reply {
    match <[_; 1]>::try_from(replies).expect("one answer") {
        [Message::Control(reply)] => reply,
        [other] => panic!("{other:?}"),
    }
}

fn send(files: &mut Files<Fake>, id: Id, offset: u64, bytes: &[u8]) -> Vec<Message<Reply>> {
    files.handle(Message::Data(Data {
        id,
        offset,
        bytes: bytes.to_vec(),
    }))
}

#[test]
fn a_file_to_the_device_resumes_and_gets_a_free_name() {
    let scratch = Scratch::new();
    let content: Vec<u8> = (0..100_000u32).map(|n| n as u8).collect();
    let mut f = files(&scratch, Fake::default());
    let upload = Request::Upload {
        id: id(1),
        name: "jelentés.txt".into(),
        size: content.len() as u64,
    };
    assert_eq!(
        control(f.handle(Message::Control(upload.clone()))),
        Reply::UploadReady {
            id: id(1),
            offset: 0
        }
    );
    let first = 3 * CHUNK;
    for (index, piece) in content[..first].chunks(CHUNK).enumerate() {
        send(&mut f, id(1), (index * CHUNK) as u64, piece);
    }
    // The session ends here; a new one resumes from what was written.
    drop(f);
    let mut f = files(&scratch, Fake::default());
    assert_eq!(
        control(f.handle(Message::Control(upload.clone()))),
        Reply::UploadReady {
            id: id(1),
            offset: first as u64
        }
    );
    // Out of order data ends the transfer.
    assert_eq!(
        control(send(&mut f, id(1), 0, &content[..10])),
        Reply::Failed {
            id: Some(id(1)),
            operation: None,
            problem: Problem::Invalid
        }
    );
    f.handle(Message::Control(upload.clone()));
    let mut acks = Vec::new();
    for (index, piece) in content[first..].chunks(CHUNK).enumerate() {
        acks.extend(send(&mut f, id(1), (first + index * CHUNK) as u64, piece));
    }
    assert_eq!(
        acks.last(),
        Some(&Message::Control(Reply::UploadAck {
            id: id(1),
            offset: content.len() as u64
        }))
    );
    fs::write(scratch.0.join("MIDAS").join("jelentés.txt"), b"older").unwrap();
    assert_eq!(
        control(f.handle(Message::Control(Request::UploadEnd { id: id(1) }))),
        Reply::UploadDone {
            id: id(1),
            name: "jelentés (2).txt".into(),
            size: content.len() as u64,
            sha256: digest(&content),
        }
    );
    assert_eq!(
        fs::read(scratch.0.join("MIDAS").join("jelentés (2).txt")).unwrap(),
        content
    );
    assert!(!f.partial(id(1)).exists());
}

#[test]
fn a_file_to_the_device_needs_room_and_can_be_cancelled() {
    let scratch = Scratch::new();
    let mut f = files(
        &scratch,
        Fake {
            free: Some(10),
            ..Fake::default()
        },
    );
    let upload = |size| {
        Message::Control(Request::Upload {
            id: id(2),
            name: "CON.txt".into(),
            size,
        })
    };
    assert_eq!(
        control(f.handle(upload(11))),
        Reply::Failed {
            id: Some(id(2)),
            operation: None,
            problem: Problem::NoSpace
        }
    );
    f.handle(upload(4));
    send(&mut f, id(2), 0, b"ab");
    assert!(f.partial(id(2)).exists());
    assert!(
        f.handle(Message::Control(Request::Cancel { id: id(2) }))
            .is_empty()
    );
    assert!(!f.partial(id(2)).exists());
    // Ending early is refused; a reserved device name is never used as is.
    f.handle(upload(2));
    send(&mut f, id(2), 0, b"a");
    assert!(matches!(
        control(f.handle(Message::Control(Request::UploadEnd { id: id(2) }))),
        Reply::Failed {
            problem: Problem::Invalid,
            ..
        }
    ));
    assert_eq!(
        control(f.handle(upload(1))),
        Reply::UploadReady {
            id: id(2),
            offset: 1
        }
    );
    assert!(matches!(
        control(f.handle(Message::Control(Request::UploadEnd { id: id(2) }))),
        Reply::UploadDone { name, .. } if name == "_CON.txt"
    ));
}

#[test]
fn a_file_from_the_device_waits_for_acknowledgements_and_resumes() {
    let scratch = Scratch::new();
    let content: Vec<u8> = (0..(WINDOW as usize + 3 * CHUNK + 5))
        .map(|n| (n * 7) as u8)
        .collect();
    let path = scratch.0.join("nagy.bin");
    fs::write(&path, &content).unwrap();
    let metadata = fs::metadata(&path).unwrap();
    let modified = seconds(metadata.modified());
    let mut f = files(&scratch, Fake::default());
    let download = |offset, modified| {
        Message::Control(Request::Download {
            id: id(3),
            path: path.to_str().unwrap().into(),
            size: content.len() as u64,
            modified,
            offset,
        })
    };
    assert!(matches!(
        control(f.handle(download(0, modified + 1))),
        Reply::Failed {
            problem: Problem::Changed,
            ..
        }
    ));
    let first = f.handle(download(0, modified));
    // Exactly one window goes out before the browser acknowledges anything.
    assert_eq!(first.len() as u64, WINDOW / CHUNK as u64);
    let mut received = Vec::new();
    for message in &first {
        let Message::Data(data) = message else {
            panic!("{message:?}")
        };
        assert_eq!(data.offset, received.len() as u64);
        received.extend_from_slice(&data.bytes);
    }
    // The session breaks off after part of it; the next one resumes and gets the same digest.
    let resumed_at = received.len() as u64 - CHUNK as u64;
    received.truncate(resumed_at as usize);
    let mut f = files(&scratch, Fake::default());
    let mut messages = f.handle(download(resumed_at, modified));
    let mut done = None;
    while done.is_none() {
        let mut acked = None;
        for message in messages.drain(..) {
            match message {
                Message::Data(data) => {
                    assert_eq!(data.offset, received.len() as u64);
                    received.extend_from_slice(&data.bytes);
                    acked = Some(data.offset + data.bytes.len() as u64);
                }
                Message::Control(reply) => done = Some(reply),
            }
        }
        if done.is_none() {
            messages = f.handle(Message::Control(Request::Ack {
                id: id(3),
                offset: acked.expect("progress"),
            }));
        }
    }
    assert_eq!(received, content);
    assert_eq!(
        done,
        Some(Reply::DownloadDone {
            id: id(3),
            size: content.len() as u64,
            sha256: digest(&content),
        })
    );
    // An acknowledgement beyond what was sent ends a transfer.
    f.handle(download(0, modified));
    assert!(matches!(
        control(f.handle(Message::Control(Request::Ack {
            id: id(3),
            offset: WINDOW + 1
        }))),
        Reply::Failed {
            problem: Problem::Invalid,
            ..
        }
    ));
}

#[test]
fn listings_show_folders_first_and_only_local_paths() {
    let scratch = Scratch::new();
    fs::create_dir(scratch.0.join("Zöld mappa")).unwrap();
    fs::write(scratch.0.join("alma.txt"), b"12345").unwrap();
    fs::write(scratch.0.join("Béka.txt"), b"1").unwrap();
    let mut f = files(&scratch, Fake::default());
    let Reply::Listing {
        path,
        parent,
        entries,
        truncated,
    } = control(f.handle(Message::Control(Request::List { path: None })))
    else {
        panic!("listing")
    };
    assert_eq!(path, scratch.0.to_str().unwrap());
    assert!(parent.is_some());
    assert!(!truncated);
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Zöld mappa", "alma.txt", "Béka.txt"]);
    assert_eq!(entries[1].size, 5);
    assert!(entries[0].dir);
    for refused in [r"\\server\share", r"\\?\C:\x", "relative", r"C:\a\..\b"] {
        assert_eq!(
            control(f.handle(Message::Control(Request::List {
                path: Some(refused.into())
            }))),
            Reply::Failed {
                id: None,
                operation: Some(Operation::List),
                problem: Problem::Invalid
            },
            "{refused}"
        );
    }
}

#[test]
fn only_local_drive_paths_are_taken() {
    for (text, windows, taken) in [
        (r"C:\Users\a\b.txt", true, true),
        (r"d:\", true, true),
        (r"\\server\share\x", true, false),
        (r"\\?\C:\x", true, false),
        (r"\\.\PhysicalDrive0", true, false),
        (r"C:\a\file.txt:secret", true, false),
        (r"C:\a\..\b", true, false),
        (r"C:/a", true, false),
        ("C:", true, false),
        ("/home/a/b", false, true),
        ("/home/../etc", false, false),
        ("home/a", false, false),
    ] {
        assert_eq!(local_path_for(text, windows).is_some(), taken, "{text}");
    }
}

#[test]
fn received_names_suit_every_file_system() {
    for (name, clean) in [
        ("jelentés.pdf", "jelentés.pdf"),
        ("a<b>c?.txt", "a_b_c_.txt"),
        ("név. ", "név"),
        ("...", "file"),
        ("nul.txt", "_nul.txt"),
        ("COM1", "_COM1"),
        ("COMPUTER.txt", "COMPUTER.txt"),
        (".0011.midas-part", ".0011.midas-part_"),
    ] {
        assert_eq!(file_name(name), clean, "{name}");
    }
}

#[test]
fn the_clipboard_moves_only_as_text_within_its_limit() {
    let scratch = Scratch::new();
    let mut f = files(&scratch, Fake::default());
    assert_eq!(
        control(f.handle(Message::Control(Request::ClipboardGet))),
        Reply::Failed {
            id: None,
            operation: Some(Operation::ClipboardGet),
            problem: Problem::Empty
        }
    );
    assert_eq!(
        control(f.handle(Message::Control(Request::ClipboardSet {
            text: "árvíztűrő".into()
        }))),
        Reply::ClipboardSet
    );
    assert_eq!(
        control(f.handle(Message::Control(Request::ClipboardGet))),
        Reply::Clipboard {
            text: "árvíztűrő".into()
        }
    );
    f.platform.clipboard = Some("x".repeat(CLIPBOARD_LIMIT + 1));
    assert!(matches!(
        control(f.handle(Message::Control(Request::ClipboardGet))),
        Reply::Failed {
            problem: Problem::TooLarge,
            ..
        }
    ));
}
