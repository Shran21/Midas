use super::*;

const ALL: Rights = Rights {
    clipboard: true,
    upload: true,
    download: true,
};
const ID: &str = "00112233445566778899aabbccddeeff";

fn control(text: &str, rights: Rights) -> Option<Request> {
    match read_request(text.as_bytes(), rights)? {
        Message::Control(request) => Some(request),
        Message::Data(_) => panic!("{text}"),
    }
}

#[test]
fn browser_requests_are_read_strictly_and_only_with_their_right() {
    let id = Id::parse(ID).unwrap();
    let cases = [
        (
            r#"{"type":"clipboard_set","text":"árvíztűrő"}"#,
            Request::ClipboardSet {
                text: "árvíztűrő".into(),
            },
            Rights {
                clipboard: true,
                ..Rights::NONE
            },
        ),
        (
            r#"{"type":"clipboard_get"}"#,
            Request::ClipboardGet,
            Rights {
                clipboard: true,
                ..Rights::NONE
            },
        ),
        (
            &format!(r#"{{"type":"upload","id":"{ID}","name":"jelentés.pdf","size":5}}"#),
            Request::Upload {
                id,
                name: "jelentés.pdf".into(),
                size: 5,
            },
            Rights {
                upload: true,
                ..Rights::NONE
            },
        ),
        (
            r#"{"type":"list"}"#,
            Request::List { path: None },
            Rights {
                download: true,
                ..Rights::NONE
            },
        ),
        (
            &format!(
                r#"{{"type":"download","id":"{ID}","path":"C:\\Users\\a\\x.txt","size":9,"modified":1,"offset":4}}"#
            ),
            Request::Download {
                id,
                path: r"C:\Users\a\x.txt".into(),
                size: 9,
                modified: 1,
                offset: 4,
            },
            Rights {
                download: true,
                ..Rights::NONE
            },
        ),
    ];
    for (text, expected, needed) in cases {
        assert_eq!(control(text, needed).as_ref(), Some(&expected), "{text}");
        assert_eq!(control(text, ALL), Some(expected.clone()));
        let others = Rights {
            clipboard: !needed.clipboard,
            upload: !needed.upload,
            download: !needed.download,
        };
        assert_eq!(control(text, others), None, "{text} without its right");
        // What reaches the worker reads back the same.
        let forwarded = request_message(&Message::Control(expected.clone()));
        assert_eq!(
            read_request(&forwarded, needed),
            Some(Message::Control(expected))
        );
    }
    for refused in [
        r#"{"type":"clipboard_set","text":""}"#.to_owned(),
        r#"{"type":"clipboard_set","text":"a\u0000b"}"#.to_owned(),
        format!(
            r#"{{"type":"clipboard_set","text":"{}"}}"#,
            "x".repeat(CLIPBOARD_LIMIT + 1)
        ),
        format!(r#"{{"type":"upload","id":"{ID}","name":"..\\boot.ini","size":1}}"#),
        format!(r#"{{"type":"upload","id":"{ID}","name":"a/b","size":1}}"#),
        format!(
            r#"{{"type":"upload","id":"{ID}","name":"x","size":{}}}"#,
            FILE_LIMIT + 1
        ),
        format!(
            r#"{{"type":"upload","id":"{}","name":"x","size":1}}"#,
            &ID[..30]
        ),
        format!(
            r#"{{"type":"upload","id":"{}","name":"x","size":1}}"#,
            ID.to_uppercase()
        ),
        format!(r#"{{"type":"upload","id":"{ID}","name":"x","size":1,"extra":1}}"#),
        format!(
            r#"{{"type":"download","id":"{ID}","path":"C:\\x","size":3,"modified":0,"offset":4}}"#
        ),
        r#"{"type":"list","path":""}"#.to_owned(),
        r#"{"type":"run","command":"calc"}"#.to_owned(),
        "not json".to_owned(),
    ] {
        assert_eq!(read_request(refused.as_bytes(), ALL), None, "{refused}");
    }
    let cancel = format!(r#"{{"type":"cancel","id":"{ID}"}}"#);
    assert!(control(&cancel, ALL).is_some());
    assert!(
        control(
            &cancel,
            Rights {
                clipboard: true,
                ..Rights::NONE
            }
        )
        .is_none()
    );
}

#[test]
fn file_data_is_bounded_and_flows_one_way_each() {
    let data = Data {
        id: Id::parse(ID).unwrap(),
        offset: 7,
        bytes: vec![1, 2, 3],
    };
    let up = request_message(&Message::Data(data.clone()));
    assert_eq!(up.len(), DATA_HEADER + 3);
    assert_eq!(read_request(&up, ALL), Some(Message::Data(data.clone())));
    assert_eq!(
        read_request(
            &up,
            Rights {
                download: true,
                clipboard: true,
                ..Rights::NONE
            }
        ),
        None
    );
    // Data towards the device is never taken as an answer, nor the other way round.
    assert_eq!(read_reply(&up, ALL), None);
    let down = reply_message(&Message::Data(data.clone()));
    assert_eq!(read_reply(&down, ALL), Some(Message::Data(data.clone())));
    assert_eq!(read_request(&down, ALL), None);
    for bytes in [Vec::new(), vec![0; CHUNK + 1]] {
        let message = request_message(&Message::Data(Data {
            bytes,
            ..data.clone()
        }));
        assert_eq!(read_request(&message, ALL), None);
    }
    let beyond = request_message(&Message::Data(Data {
        offset: FILE_LIMIT,
        ..data.clone()
    }));
    assert_eq!(read_request(&beyond, ALL), None);
    assert_eq!(read_request(&up[..DATA_HEADER], ALL), None);
}

#[test]
fn worker_answers_are_checked_against_the_grant() {
    let id = Id::parse(ID).unwrap();
    let done = Reply::UploadDone {
        id,
        name: "jelentés (1).pdf".into(),
        size: 5,
        sha256: Hex([0xab; 32]),
    };
    let message = reply_message(&Message::Control(done.clone()));
    assert!(String::from_utf8_lossy(&message).contains(&"ab".repeat(32)));
    assert_eq!(read_reply(&message, ALL), Some(Message::Control(done)));
    assert_eq!(
        read_reply(
            &message,
            Rights {
                upload: false,
                ..ALL
            }
        ),
        None
    );
    let clipboard = reply_message(&Message::Control(Reply::Clipboard { text: "x".into() }));
    assert_eq!(
        read_reply(
            &clipboard,
            Rights {
                clipboard: false,
                ..ALL
            }
        ),
        None
    );
    let listing = |entries: Vec<Entry>| {
        reply_message(&Message::Control(Reply::Listing {
            path: r"C:\Users\a".into(),
            parent: Some(r"C:\Users".into()),
            entries,
            truncated: false,
        }))
    };
    let entry = Entry {
        name: "Dokumentumok".into(),
        dir: true,
        size: 0,
        modified: 1,
    };
    assert!(read_reply(&listing(vec![entry.clone()]), ALL).is_some());
    let bad = Entry {
        name: "a\\b".into(),
        ..entry.clone()
    };
    assert_eq!(read_reply(&listing(vec![bad]), ALL), None);
    assert_eq!(
        read_reply(&listing(vec![entry; LISTING_LIMIT + 1]), ALL),
        None
    );
    // A failure names either a transfer or an operation, and needs the matching right.
    for (failed, rights, allowed) in [
        (
            Reply::Failed {
                id: None,
                operation: Some(Operation::ClipboardGet),
                problem: Problem::Empty,
            },
            Rights {
                clipboard: true,
                ..Rights::NONE
            },
            true,
        ),
        (
            Reply::Failed {
                id: None,
                operation: Some(Operation::List),
                problem: Problem::Denied,
            },
            Rights {
                clipboard: true,
                ..Rights::NONE
            },
            false,
        ),
        (
            Reply::Failed {
                id: Some(id),
                operation: Some(Operation::List),
                problem: Problem::Denied,
            },
            ALL,
            false,
        ),
        (
            Reply::Failed {
                id: Some(id),
                operation: None,
                problem: Problem::NoSpace,
            },
            Rights {
                upload: true,
                ..Rights::NONE
            },
            true,
        ),
    ] {
        let message = reply_message(&Message::Control(failed.clone()));
        assert_eq!(
            read_reply(&message, rights).is_some(),
            allowed,
            "{failed:?}"
        );
    }
}

#[test]
fn the_ledger_reports_starts_ends_and_breaks_without_content() {
    let id = Id::parse(ID).unwrap();
    let mut ledger = Ledger::default();
    ledger.request(&Request::Upload {
        id,
        name: "jelentés.pdf".into(),
        size: 5,
    });
    // A resumed start within the same session is reported once.
    ledger.request(&Request::Upload {
        id,
        name: "jelentés.pdf".into(),
        size: 5,
    });
    ledger.reply(&Reply::UploadDone {
        id,
        name: "jelentés (2).pdf".into(),
        size: 5,
        sha256: Hex([1; 32]),
    });
    ledger.request(&Request::ClipboardSet {
        text: "titok".into(),
    });
    ledger.reply(&Reply::ClipboardSet);
    ledger.request(&Request::Download {
        id: Hex([2; 16]),
        path: r"C:\Users\a\x.txt".into(),
        size: 9,
        modified: 1,
        offset: 0,
    });
    ledger.close();
    let reports: Vec<_> = std::iter::from_fn(|| ledger.take()).collect();
    let outcomes: Vec<_> = reports.iter().map(|r| (r.kind, r.outcome)).collect();
    assert_eq!(
        outcomes,
        [
            (Kind::Upload, Outcome::Started),
            (Kind::Upload, Outcome::Completed),
            (Kind::ClipboardToDevice, Outcome::Completed),
            (Kind::Download, Outcome::Started),
            (Kind::Download, Outcome::Interrupted),
        ]
    );
    assert_eq!(reports[1].name.as_deref(), Some("jelentés (2).pdf"));
    assert_eq!(reports[3].name.as_deref(), Some("x.txt"));
    assert_eq!(reports[2].name, None);
    // Between processes and to the server they read back the same, and nothing else passes.
    let message = reports_message(&reports);
    assert!(!String::from_utf8_lossy(&message).contains("titok"));
    assert_eq!(read_reports(&message), Some(reports.clone()));
    let request = Uuid::new_v4();
    let mut wire = reports[1].wire(request);
    assert_eq!(wire.request, request.as_bytes());
    assert_eq!(Report::from_wire(&wire), Some(reports[1].clone()));
    wire.problem = "boom".into();
    assert_eq!(Report::from_wire(&wire), None);
    let mut clipboard = reports[2].wire(request);
    clipboard.size = Some(5);
    assert_eq!(Report::from_wire(&clipboard), None, "no clipboard length");
    let mut digest = reports[0].wire(request);
    digest.sha256 = vec![0; 32];
    assert_eq!(
        Report::from_wire(&digest),
        None,
        "a digest only when completed"
    );
    assert_eq!(read_reports(&message[..message.len() - 1]), None);
}
