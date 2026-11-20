use std::net::Ipv4Addr;

use super::*;
use crate::media::{
    codec::Profile,
    testing::{connect, keyframe, software},
};

#[test]
fn frames_reach_the_browser_and_data_flows_both_ways() {
    let (mut browser, mut peer, _) = connect(software());
    assert_eq!(
        peer.choice().map(|choice| choice.profile),
        Some(Profile::ConstrainedBaseline)
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut sent = 0;
    let mut received = Vec::new();
    while browser.frames.is_empty() || received.is_empty() {
        assert!(Instant::now() < deadline, "no frame or message");
        if sent < 30 && peer.send_frame(&keyframe(sent), Instant::now()).unwrap() {
            sent += 1;
        }
        if browser.messages.is_empty() {
            browser.send(br#"{"type":"keyframe"}"#);
        }
        peer.send_data(br#"{"type":"stats"}"#).unwrap();
        browser.step();
        peer.drive(Instant::now() + Duration::from_millis(5))
            .unwrap();
        while let Some(event) = peer.next_event() {
            if let PeerEvent::Data(data) = event {
                received.push(data);
            }
        }
    }
    let frame = &browser.frames[0];
    assert!(
        frame.windows(4).any(|w| w == [0, 0, 0, 1]) || frame.windows(3).any(|w| w == [0, 0, 1])
    );
    assert!(
        frame.len() >= 3000,
        "fragmented frame reassembled: {}",
        frame.len()
    );
    assert_eq!(received[0], br#"{"type":"keyframe"}"#);
    assert!(browser.messages.iter().any(|m| m == br#"{"type":"stats"}"#));
    peer.close();
    assert!(peer.is_closed());
    assert_eq!(peer.next_event(), Some(PeerEvent::Closed));
    assert!(matches!(peer.drive(Instant::now()), Err(PeerError::Closed)));
}

#[test]
fn offers_without_video_or_codec_are_refused_or_reported() {
    crypto();
    let socket = || UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let hosts = [IpAddr::V4(Ipv4Addr::LOCALHOST)];
    assert!(matches!(
        answer("not sdp", socket(), &hosts, software(), Instant::now()),
        Err(PeerError::Offer)
    ));
    let mut rtc = RtcConfig::new().build(Instant::now());
    let mut change = rtc.sdp_api();
    change.add_channel("control".into());
    let (offer, _) = change.apply().unwrap();
    assert!(matches!(
        answer(
            &offer.to_sdp_string(),
            socket(),
            &hosts,
            software(),
            Instant::now()
        ),
        Err(PeerError::NoVideo)
    ));
    let (_browser, mut peer, events) = connect(Vec::new());
    assert!(peer.choice().is_none());
    assert!(events.contains(&PeerEvent::NoCodec), "{events:?}");
    assert!(!peer.send_frame(&keyframe(0), Instant::now()).unwrap());
}
