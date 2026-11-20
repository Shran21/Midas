use std::thread;

use super::*;

#[test]
fn messages_round_trip_and_integrity_detects_any_change() {
    let id = transaction();
    let key = long_term_key("1700000000:request", b"midas", "secret");
    let message = encode(
        ALLOCATE | REQUEST,
        &id,
        &[
            (REQUESTED_TRANSPORT, &[UDP, 0, 0, 0]),
            (USERNAME, b"1700000000:request"),
        ],
        Some(&key),
    );
    let decoded = decode(&message).unwrap();
    assert_eq!(decoded.kind, ALLOCATE | REQUEST);
    assert_eq!(decoded.transaction, id);
    assert_eq!(
        decoded.attribute(USERNAME),
        Some(&b"1700000000:request"[..])
    );
    assert!(authentic(&message, &key));
    assert!(!authentic(
        &message,
        &long_term_key("other", b"midas", "secret")
    ));
    let mut changed = message.clone();
    changed[HEADER + 9] ^= 1;
    assert!(!authentic(&changed, &key));
    assert!(decode(&message[..message.len() - 4]).is_none());
    let mut foreign = message.clone();
    foreign[4] ^= 1;
    assert!(decode(&foreign).is_none());
}

#[test]
fn xor_addresses_round_trip_for_both_families() {
    let id = transaction();
    for address in [
        "192.0.2.10:3478".parse::<SocketAddr>().unwrap(),
        "[2001:db8::1]:49152".parse().unwrap(),
    ] {
        assert_eq!(
            read_xor_address(&xor_address(address, &id), &id),
            Some(address)
        );
    }
    assert_eq!(read_xor_address(&[0, 1, 0], &id), None);
}

/// A scripted relay on loopback: it checks the client's credentials like a real one.
fn relay(password: &'static str) -> (SocketAddr, thread::JoinHandle<Vec<u16>>) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let address = socket.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let key = long_term_key("user", b"midas", password);
        let mut seen = Vec::new();
        let mut buffer = [0u8; 2048];
        let relayed = "198.51.100.7:50000".parse().unwrap();
        while let Ok((length, client)) = socket.recv_from(&mut buffer) {
            let raw = buffer[..length].to_vec();
            let Some(message) = decode(&raw) else { break };
            seen.push(message.kind);
            let id = message.transaction;
            let reply = match message.kind {
                kind if kind == ALLOCATE | REQUEST && message.attribute(USERNAME).is_none() => {
                    encode(
                        ALLOCATE | FAILURE,
                        &id,
                        &[
                            (ERROR_CODE, &[0, 0, 4, 1]),
                            (REALM, b"midas"),
                            (NONCE, b"n1"),
                        ],
                        None,
                    )
                }
                kind if kind == ALLOCATE | REQUEST => {
                    if !authentic(&raw, &key) {
                        encode(
                            ALLOCATE | FAILURE,
                            &id,
                            &[(ERROR_CODE, &[0, 0, 4, 1])],
                            None,
                        )
                    } else {
                        let relayed = xor_address(relayed, &id);
                        let mapped = xor_address(client, &id);
                        let lifetime = 600u32.to_be_bytes();
                        encode(
                            ALLOCATE | SUCCESS,
                            &id,
                            &[
                                (XOR_RELAYED_ADDRESS, &relayed),
                                (XOR_MAPPED_ADDRESS, &mapped),
                                (LIFETIME, &lifetime),
                            ],
                            Some(&key),
                        )
                    }
                }
                // Internal destinations are forbidden; the peer below is allowed.
                kind if kind == CREATE_PERMISSION | REQUEST => {
                    let peer = read_xor_address(message.attribute(XOR_PEER_ADDRESS).unwrap(), &id)
                        .unwrap();
                    if peer.ip().is_loopback() {
                        encode(
                            CREATE_PERMISSION | FAILURE,
                            &id,
                            &[(ERROR_CODE, &[0, 0, 4, 3])],
                            None,
                        )
                    } else {
                        encode(CREATE_PERMISSION | SUCCESS, &id, &[], Some(&key))
                    }
                }
                kind if kind == SEND | INDICATION => {
                    let peer = read_xor_address(message.attribute(XOR_PEER_ADDRESS).unwrap(), &id)
                        .unwrap();
                    let payload = message.attribute(DATA_ATTRIBUTE).unwrap().to_vec();
                    // Echo the payload back as if the peer had answered.
                    let id = transaction();
                    let peer = xor_address(peer, &id);
                    encode(
                        DATA | INDICATION,
                        &id,
                        &[(XOR_PEER_ADDRESS, &peer), (DATA_ATTRIBUTE, &payload)],
                        None,
                    )
                }
                _ => break,
            };
            socket.send_to(&reply, client).unwrap();
        }
        seen
    });
    (address, handle)
}

#[test]
fn allocation_relays_to_permitted_peers_and_drops_refused_ones() {
    let (address, relay) = relay("secret");
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server = Server {
        address,
        username: "user".into(),
        password: "secret".into(),
    };
    let mut allocation =
        allocate(&socket, &server, Instant::now() + Duration::from_secs(5)).unwrap();
    assert_eq!(allocation.relayed, "198.51.100.7:50000".parse().unwrap());
    assert_eq!(allocation.mapped, Some(socket.local_addr().unwrap()));
    let now = Instant::now();
    let read = |allocation: &mut Allocation| {
        let mut buffer = [0u8; 2048];
        let (length, source) = socket.recv_from(&mut buffer).unwrap();
        allocation.receive(source, &buffer[..length], Instant::now())
    };

    let peer: SocketAddr = "203.0.113.9:4000".parse().unwrap();
    let out = allocation.send(peer, b"hello", now);
    assert_eq!(out.len(), 2, "a permission, then the packet");
    for message in &out {
        socket.send_to(message, address).unwrap();
    }
    assert_eq!(read(&mut allocation).0, Received::Handled);
    assert_eq!(
        read(&mut allocation).0,
        Received::Data(peer, b"hello".to_vec())
    );
    assert_eq!(
        allocation.send(peer, b"again", now).len(),
        1,
        "permission kept"
    );

    let internal: SocketAddr = "127.0.0.1:9".parse().unwrap();
    for message in allocation.send(internal, b"x", now) {
        socket.send_to(&message, address).unwrap();
    }
    assert_eq!(read(&mut allocation).0, Received::Handled);
    // The scripted relay echoes the indication even so; without a permission it is ignored.
    assert_eq!(read(&mut allocation).0, Received::Ignored);
    assert!(allocation.refused(internal.ip()));
    assert!(allocation.send(internal, b"x", now).is_empty());

    // A packet that does not come from the relay is never taken as relayed data.
    let stranger = UdpSocket::bind("127.0.0.1:0").unwrap();
    let id = transaction();
    let forged = encode(
        DATA | INDICATION,
        &id,
        &[
            (XOR_PEER_ADDRESS, &xor_address(peer, &id)),
            (DATA_ATTRIBUTE, b"forged"),
        ],
        None,
    );
    assert_eq!(
        allocation
            .receive(stranger.local_addr().unwrap(), &forged, now)
            .0,
        Received::Ignored
    );
    drop(socket);
    drop(stranger);
    let seen = relay.join().unwrap();
    assert_eq!(&seen[..2], &[ALLOCATE | REQUEST, ALLOCATE | REQUEST]);
}

#[test]
fn wrong_credentials_are_refused() {
    let (address, relay) = relay("other");
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server = Server {
        address,
        username: "user".into(),
        password: "secret".into(),
    };
    assert_eq!(
        allocate(&socket, &server, Instant::now() + Duration::from_secs(5)).err(),
        Some(TurnError::Refused(401))
    );
    drop(socket);
    let _ = relay.join();
}

#[test]
fn the_allocation_is_refreshed_before_it_lapses() {
    let (address, relay) = relay("secret");
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let server = Server {
        address,
        username: "user".into(),
        password: "secret".into(),
    };
    let mut allocation =
        allocate(&socket, &server, Instant::now() + Duration::from_secs(5)).unwrap();
    let now = Instant::now();
    assert!(allocation.due(now).is_empty());
    assert!(allocation.next_due() > now + Duration::from_secs(500));
    let late = now + Duration::from_secs(550);
    let refresh = allocation.due(late);
    assert_eq!(refresh.len(), 1);
    assert_eq!(decode(&refresh[0]).unwrap().kind, REFRESH | REQUEST);
    assert!(allocation.due(late).is_empty(), "one refresh at a time");
    drop(socket);
    let _ = relay.join();
}

#[test]
fn only_udp_turn_urls_are_used_by_the_agent() {
    let relay = |urls: &[&str]| crate::remote::transport::Relay {
        urls: urls.iter().map(|url| (*url).to_owned()).collect(),
        username: "1:request".into(),
        credential: "secret".into(),
    };
    let chosen = udp_relay(&[relay(&[
        "turns:127.0.0.1:443?transport=tcp",
        "turn:127.0.0.1:3478?transport=udp",
    ])])
    .unwrap();
    assert_eq!(chosen.address, "127.0.0.1:3478".parse().unwrap());
    assert_eq!(chosen.username, "1:request");
    assert_eq!(
        udp_relay(&[relay(&["turn:127.0.0.1"])]).unwrap().address,
        "127.0.0.1:3478".parse().unwrap()
    );
    assert_eq!(
        udp_relay(&[relay(&["turn:[::1]:3479"])]).unwrap().address,
        "[::1]:3479".parse().unwrap()
    );
    assert!(
        udp_relay(&[relay(&[
            "turn:127.0.0.1:3478?transport=tcp",
            "stun:127.0.0.1"
        ])])
        .is_none()
    );
    assert!(udp_relay(&[]).is_none());
}

#[test]
fn the_trial_credential_uses_standard_base64() {
    assert_eq!(base64(b"Man"), "TWFu");
    assert_eq!(base64(b"Ma"), "TWE=");
    assert_eq!(base64(b"M"), "TQ==");
    assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let value = chunk.iter().enumerate().fold(0u32, |value, (i, byte)| {
            value | u32::from(*byte) << (16 - 8 * i)
        });
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                TABLE[(value >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// What the MIDAS server issues: the expiry and a name, signed with the relay's shared secret.
fn issued(address: SocketAddr, secret: &str, expiry: i64) -> Server {
    let username = format!("{expiry}:trial");
    let mut mac = <Hmac<Sha1> as KeyInit>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(username.as_bytes());
    Server {
        address,
        username,
        password: base64(&mac.finalize().into_bytes()),
    }
}

/// Reads the relay until `done` accepts what came, or panics after a few seconds.
fn until(
    socket: &UdpSocket,
    allocation: &mut Allocation,
    mut done: impl FnMut(&Received, &Allocation) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut buffer = [0u8; 2048];
    socket
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    while Instant::now() < deadline {
        if let Ok((length, source)) = socket.recv_from(&mut buffer) {
            let (received, replies) = allocation.receive(source, &buffer[..length], Instant::now());
            for reply in replies {
                socket.send_to(&reply, allocation.server()).unwrap();
            }
            if done(&received, allocation) {
                return;
            }
        }
    }
    panic!("the relay did not answer as expected");
}

#[test]
#[ignore = "needs a real relay; run by .github/scripts/check-turn.py"]
fn relay_trial() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name}"));
    let server: SocketAddr = var("MIDAS_TURN_TRIAL_SERVER").parse().unwrap();
    let server_v6: SocketAddr = var("MIDAS_TURN_TRIAL_SERVER_V6").parse().unwrap();
    let secret = var("MIDAS_TURN_TRIAL_SECRET");
    let peer_ip: IpAddr = var("MIDAS_TURN_TRIAL_PEER").parse().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // The relay may still be starting; a valid credential opens an allocation once it is up.
    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let started = Instant::now();
    let mut allocation = loop {
        match allocate(
            &socket,
            &issued(server, &secret, now + 300),
            Instant::now() + Duration::from_secs(2),
        ) {
            Ok(allocation) => break allocation,
            Err(error) if started.elapsed() < Duration::from_secs(30) => {
                println!("relay not ready yet: {error:?}");
            }
            Err(error) => panic!("no allocation: {error:?}"),
        }
    };
    println!(
        "relayed {} mapped {:?}",
        allocation.relayed, allocation.mapped
    );
    assert_eq!(allocation.relayed.ip(), peer_ip);

    // Short-lived credentials: an expired one, or one signed with another secret, opens nothing.
    let other = UdpSocket::bind("0.0.0.0:0").unwrap();
    assert_eq!(
        allocate(
            &other,
            &issued(server, &secret, now - 60),
            Instant::now() + Duration::from_secs(3)
        )
        .err(),
        Some(TurnError::Refused(401))
    );
    assert_eq!(
        allocate(
            &other,
            &issued(server, "another-secret-entirely", now + 300),
            Instant::now() + Duration::from_secs(3)
        )
        .err(),
        Some(TurnError::Refused(401))
    );

    // Internal and reserved IPv4 destinations are refused.
    for blocked in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.1.1",
        "100.64.0.1",
        "192.0.2.1",
        "198.18.0.1",
        "224.0.0.1",
        "240.0.0.1",
    ] {
        let blocked: IpAddr = blocked.parse().unwrap();
        let request = allocation.permit(blocked, Instant::now());
        socket.send_to(&request, server).unwrap();
        until(&socket, &mut allocation, |_, allocation| {
            allocation.refused(blocked)
        });
        println!("refused {blocked}");
    }

    // A real round trip through the relay with the one whitelisted peer.
    let peer = UdpSocket::bind(SocketAddr::new(peer_ip, 0)).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let destination = peer.local_addr().unwrap();
    for message in allocation.send(destination, b"ping", Instant::now()) {
        socket.send_to(&message, server).unwrap();
    }
    // The first packet may outrun its permission; send again once it is in place.
    let mut buffer = [0u8; 64];
    let mut got = peer.recv_from(&mut buffer);
    if got.is_err() {
        for message in allocation.send(destination, b"ping", Instant::now()) {
            socket.send_to(&message, server).unwrap();
        }
        got = peer.recv_from(&mut buffer);
    }
    let (length, from) = got.expect("relayed packet");
    assert_eq!(&buffer[..length], b"ping");
    assert_eq!(from, allocation.relayed);
    peer.send_to(b"pong", allocation.relayed).unwrap();
    until(&socket, &mut allocation, |received, _| {
        *received == Received::Data(destination, b"pong".to_vec())
    });
    println!("relayed round trip with {destination}");
    socket
        .send_to(&allocation.release(Instant::now()), server)
        .unwrap();

    // A whole WebRTC connection whose only agent candidate is the relayed address: the agent
    // sits on loopback, so the browser on the runner's address can reach it only through the
    // relay, and a frame still arrives, DTLS-SRTP protected end to end.
    let loopback = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), server.port());
    let agent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let relay = allocate(
        &agent,
        &issued(loopback, &secret, now + 300),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("an allocation for the connection");
    assert_eq!(relay.mapped, Some(agent.local_addr().unwrap()));
    let (mut browser, mut peer, events) = crate::media::testing::connect_with(
        crate::media::testing::software(),
        peer_ip,
        agent,
        &[],
        Some(relay),
    );
    assert!(
        events.contains(&crate::media::peer::PeerEvent::Connected),
        "{events:?}"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut sequence = 0u8;
    while browser.frames.is_empty() {
        assert!(Instant::now() < deadline, "no frame over the relay");
        peer.send_frame(&crate::media::testing::keyframe(sequence), Instant::now())
            .unwrap();
        sequence = sequence.wrapping_add(1);
        browser.step();
        peer.drive(Instant::now() + Duration::from_millis(5))
            .unwrap();
    }
    println!("a frame crossed the relay");
    peer.close();

    // Internal and reserved IPv6 destinations are refused as well.
    let socket = UdpSocket::bind("[::1]:0").unwrap();
    let mut allocation = allocate(
        &socket,
        &issued(server_v6, &secret, now + 300),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("an IPv6 allocation");
    for blocked in [
        "::1",
        "fc00::1",
        "fd12:3456::1",
        "fe80::1",
        "::ffff:10.0.0.1",
        "64:ff9b::a00:1",
        "2001:db8::1",
        "ff02::1",
    ] {
        let blocked: IpAddr = blocked.parse().unwrap();
        let request = allocation.permit(blocked, Instant::now());
        socket.send_to(&request, server_v6).unwrap();
        until(&socket, &mut allocation, |_, allocation| {
            allocation.refused(blocked)
        });
        println!("refused {blocked}");
    }
    socket
        .send_to(&allocation.release(Instant::now()), server_v6)
        .unwrap();
}
