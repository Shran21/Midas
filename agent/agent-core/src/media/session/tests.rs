use super::*;
use crate::media::{
    link::{desktop_message, frame_messages, sas_result_message},
    testing::{connect, keyframe, software},
};

fn encoded(sequence: u32) -> Encoded {
    Encoded {
        sequence,
        captured: u64::from(sequence) * 33_000,
        width: 1280,
        height: 720,
        keyframe: sequence == 0,
        data: keyframe(sequence as u8),
    }
}

fn commands(transport: &mut Transport) -> Vec<Command> {
    std::iter::from_fn(|| transport.next_command()).collect()
}

#[test]
fn worker_frames_reach_the_browser_and_commands_flow_back() {
    let (mut browser, peer, _) = connect(software());
    let mut transport = Transport::new(peer, Profile::Text, (1920, 1080), Instant::now());
    let first = commands(&mut transport);
    assert!(matches!(first[0], Command::Settings(_)), "{first:?}");
    assert_eq!(transport.frame_size(), (1920, 1080));

    let pointer = br#"{"type":"pointer","x":0.25,"y":0.5,"buttons":1}"#;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut sequence = 0;
    let mut inputs = Vec::new();
    let mut seen = Vec::new();
    let find = |messages: &[Vec<u8>], kind: &str| {
        messages
            .iter()
            .filter_map(|m| serde_json::from_slice::<serde_json::Value>(m).ok())
            .find(|m| m["type"] == kind)
    };
    let stats = |messages: &[Vec<u8>]| find(messages, "stats");
    while browser.frames.is_empty()
        || inputs.len() < 2
        || stats(&browser.messages).is_none()
        || find(&browser.messages, "sas").is_none()
    {
        assert!(
            Instant::now() < deadline,
            "frames {} inputs {inputs:?}",
            browser.frames.len()
        );
        if sequence < 60 {
            for message in frame_messages(&encoded(sequence)).unwrap() {
                transport.worker_message(&message).unwrap();
            }
            sequence += 1;
        }
        if inputs.is_empty() {
            browser.send(pointer);
            browser.send(br#"{"type":"profile","profile":"motion"}"#);
            browser.send(br#"{"type":"keyframe"}"#);
            browser.send(br#"{"type":"sas"}"#);
            browser.send(r#"{"type":"text","text":"őz"}"#.as_bytes());
            browser.send(br#"{"type":"pointer","x":2.0,"y":0.5,"buttons":1}"#);
            browser.send(br#"{"type":"run","command":"calc"}"#);
        }
        browser.step();
        transport
            .drive(Instant::now() + Duration::from_millis(5))
            .unwrap();
        inputs.extend(std::iter::from_fn(|| transport.next_input()));
        // The browser's channel is open once its input arrives.
        if !inputs.is_empty() && find(&browser.messages, "sas").is_none() {
            transport
                .worker_message(&sas_result_message(false))
                .unwrap();
        }
        seen.extend(commands(&mut transport));
    }
    assert_eq!(
        inputs[0],
        Input::Pointer {
            x: 0.25,
            y: 0.5,
            buttons: 1,
            modifiers: 0
        }
    );
    assert!(
        inputs.contains(&Input::Text { text: "őz".into() }),
        "{inputs:?}"
    );
    assert!(inputs.iter().all(|input| input.valid()), "{inputs:?}");
    let answer = find(&browser.messages, "sas").unwrap();
    assert_eq!(answer["result"], "unavailable");
    let report = stats(&browser.messages).unwrap();
    assert_eq!(
        (report["width"].as_u64(), report["height"].as_u64()),
        (Some(1280), Some(720))
    );
    assert_eq!(report["auto"], true);
    assert_eq!(report["desktop"], "user");
    assert!(report["network"] == "good" || report["network"] == "degraded");
    // Packets went over loopback, never through a relay.
    assert!(report["path"] == "direct" || report["path"].is_null());
    assert!(report["rtt_ms"].is_null() || report["rtt_ms"].as_u64().is_some());
    assert!(report["target_bitrate"].as_u64().is_some() && report["sent_fps"].as_f64().is_some());
    assert!(seen.contains(&Command::Keyframe));
    assert!(seen.contains(&Command::Sas));
    assert!(
        seen.iter()
            .any(|c| matches!(c, Command::Settings(s) if s.scale < 1.0 || s.fps == 30)),
        "{seen:?}"
    );
    transport.stop();
    assert!(transport.is_stopped());
    assert_eq!(commands(&mut transport).last(), Some(&Command::Stop));
    assert!(transport.drive(Instant::now()).is_err());
}

#[test]
fn a_change_of_frame_source_drops_the_unfinished_frame_and_asks_for_a_keyframe() {
    let (_browser, peer, _) = connect(software());
    let mut transport = Transport::new(peer, Profile::Text, (1920, 1080), Instant::now());
    commands(&mut transport);
    let large = Encoded {
        data: vec![7; 150_000],
        ..encoded(1)
    };
    let messages = frame_messages(&large).unwrap();
    assert!(messages.len() > 1);
    transport.worker_message(&messages[0]).unwrap();
    transport
        .worker_message(&desktop_message(Desktop::Secure))
        .unwrap();
    assert_eq!(transport.desktop(), Desktop::Secure);
    assert_eq!(commands(&mut transport), vec![Command::Keyframe]);
    // The rest of the old source's frame arrives late and is not mistaken for a new one.
    for message in &messages[1..] {
        transport.worker_message(message).unwrap();
    }
    for message in frame_messages(&encoded(0)).unwrap() {
        transport.worker_message(&message).unwrap();
    }
    transport
        .worker_message(&desktop_message(Desktop::Elevated))
        .unwrap();
    assert_eq!(commands(&mut transport), vec![Command::Keyframe]);
    transport
        .worker_message(&desktop_message(Desktop::User))
        .unwrap();
    assert!(commands(&mut transport).is_empty(), "same source");
    assert!(!transport.is_stopped());
}

#[test]
fn a_misbehaving_worker_ends_the_session() {
    let (_browser, peer, _) = connect(software());
    let mut transport = Transport::new(peer, Profile::Text, (1920, 1080), Instant::now());
    assert_eq!(transport.worker_message(&[1, 0]), Err(LinkError::Invalid));
    assert!(transport.is_stopped());
    assert!(commands(&mut transport).contains(&Command::Stop));
}

#[test]
fn only_well_formed_input_passes() {
    for (message, valid) in [
        (r#"{"type":"pointer","x":0,"y":1,"buttons":0}"#, true),
        (r#"{"type":"wheel","dx":-120,"dy":0}"#, true),
        (r#"{"type":"key","code":"KeyA","down":true}"#, true),
        (r#"{"type":"key","code":"","down":true}"#, false),
        (r#"{"type":"key","code":"Key A","down":true}"#, false),
        (r#"{"type":"pointer","x":-0.1,"y":0.5,"buttons":0}"#, false),
        (r#"{"type":"pointer","x":0.5,"y":0.5,"buttons":64}"#, false),
        (
            r#"{"type":"pointer","x":0.5,"y":0.5,"buttons":1,"modifiers":15}"#,
            true,
        ),
        (
            r#"{"type":"pointer","x":0.5,"y":0.5,"buttons":1,"modifiers":16}"#,
            false,
        ),
        (r#"{"type":"text","text":"\u0151\t\n"}"#, true),
        (r#"{"type":"text","text":""}"#, false),
        (r#"{"type":"text","text":"\u0007"}"#, false),
    ] {
        let input: Input = serde_json::from_str(message).unwrap();
        assert_eq!(input.valid(), valid, "{message}");
    }
    assert!(
        serde_json::from_str::<Input>(r#"{"type":"pointer","x":0,"y":0,"buttons":0,"extra":1}"#)
            .is_err()
    );
}

#[test]
fn transfer_messages_pass_only_with_their_right_and_answers_come_back() {
    use midas_remote_auth::Transfer as Rights;

    use crate::media::{
        link::transfer_reply_message,
        transfer::{Kind, Message, Reply, Request, read_request, reply_message},
    };
    let all = Rights {
        clipboard: true,
        upload: true,
        download: true,
    };
    let (mut browser, peer, _) = connect(software());
    let mut transport = Transport::new(peer, Profile::Text, (1920, 1080), Instant::now());
    transport.allow_transfer(Rights {
        clipboard: true,
        ..Rights::NONE
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut forwarded = Vec::new();
    while forwarded.is_empty() {
        assert!(Instant::now() < deadline, "no transfer message passed");
        browser.send_transfer(br#"{"type":"clipboard_get"}"#);
        // Downloads are not granted: never forwarded.
        browser.send_transfer(br#"{"type":"list"}"#);
        browser.step();
        transport
            .drive(Instant::now() + Duration::from_millis(5))
            .unwrap();
        forwarded.extend(commands(&mut transport).into_iter().filter_map(
            |command| match command {
                Command::Transfer(message) => Some(message),
                _ => None,
            },
        ));
    }
    assert!(
        forwarded
            .iter()
            .all(|m| read_request(m, all) == Some(Message::Control(Request::ClipboardGet)))
    );
    let answer = reply_message(&Message::Control(Reply::Clipboard {
        text: "árvíz".into(),
    }));
    transport
        .worker_message(&transfer_reply_message(&answer))
        .unwrap();
    while browser.transfers.is_empty() {
        assert!(Instant::now() < deadline, "no answer reached the browser");
        browser.step();
        transport
            .drive(Instant::now() + Duration::from_millis(5))
            .unwrap();
    }
    let received: serde_json::Value = serde_json::from_slice(&browser.transfers[0]).unwrap();
    assert_eq!(received["type"], "clipboard");
    assert_eq!(received["text"], "árvíz");
    // The audit learns that the clipboard moved, not what it held.
    let report = transport.next_report().unwrap();
    assert_eq!(
        (report.kind, report.name),
        (Kind::ClipboardFromDevice, None)
    );
    // An answer the grant does not allow means a misbehaving worker: the session ends.
    let listing = reply_message(&Message::Control(Reply::Listing {
        path: "/home".into(),
        parent: None,
        entries: Vec::new(),
        truncated: false,
    }));
    assert_eq!(
        transport.worker_message(&transfer_reply_message(&listing)),
        Err(LinkError::Invalid)
    );
    assert!(transport.is_stopped());
}
