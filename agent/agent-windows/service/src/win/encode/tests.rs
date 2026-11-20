use super::*;

/// NAL unit types in an Annex B stream.
fn units(stream: &[u8]) -> Vec<u8> {
    let mut types = Vec::new();
    let mut index = 0;
    while index + 3 < stream.len() {
        let start = if stream[index..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if stream[index..].starts_with(&[0, 0, 1]) {
            3
        } else {
            index += 1;
            continue;
        };
        index += start;
        if let Some(header) = stream.get(index) {
            types.push(header & 0x1f);
        }
    }
    types
}

fn frame(width: u32, height: u32, shift: u32) -> Vec<u8> {
    let mut data = vec![128u8; nv12_len(width, height)];
    for y in 0..height {
        for x in 0..width {
            data[(y * width + x) as usize] = (16 + (x + y + shift) % 200) as u8;
        }
    }
    data
}

#[test]
fn encodes_constrained_frames_and_keyframes_on_request() {
    let (width, height) = (320, 240);
    let mut encoder = match SoftwareH264::new(width, height, 30, 500_000) {
        Ok(encoder) => encoder,
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            println!("skipped: {error}");
            return;
        }
        Err(error) => panic!("encoder: {error}"),
    };
    let mut streams = Vec::new();
    for index in 0..12 {
        let stream = encoder
            .encode(&frame(width, height, index * 7), index == 8)
            .unwrap()
            .to_vec();
        streams.push(stream);
    }
    let seen: Vec<_> = streams.iter().map(|stream| units(stream)).collect();
    let delay = streams
        .iter()
        .position(|stream| !stream.is_empty())
        .unwrap();
    assert_eq!(delay, 0, "low-latency mode outputs every frame at once");
    let first = &streams[delay];
    let sps = first
        .windows(5)
        .position(|w| w[..4] == [0, 0, 0, 1] && w[4] & 0x1f == 7)
        .unwrap();
    assert_eq!(first[sps + 5], 66, "baseline profile");
    for wanted in [7, 8, 5] {
        assert!(seen[delay].contains(&wanted), "{seen:?}");
    }
    assert!(
        seen[delay + 1..8].iter().flatten().any(|&t| t == 1),
        "{seen:?}"
    );
    assert!(
        seen[8..].iter().flatten().any(|&t| t == 5),
        "forced keyframe: {seen:?}"
    );
    encoder.set_bitrate(250_000).unwrap();
    assert!(
        !encoder
            .encode(&frame(width, height, 99), false)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        encoder.encode(&[0; 10], false).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(SoftwareH264::new(321, 240, 30, 500_000).is_err());
}
