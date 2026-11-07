use axum_server::{Handle, tls_rustls::RustlsConfig};
use futures_util::{SinkExt, StreamExt};
use midas_agent_core::{
    backoff::BackoffPolicy,
    connection::{Reason, Settings, Status, run},
};
use midas_protocol::{Body, Hello, Ping, RejectReason, VersionRange};
use midas_server::{
    agent::AgentProtocol,
    agent_router_with,
    transport::{pem_resolver, server_config},
};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer,
    KeyPair,
};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{PrivatePkcs8KeyDer, ServerName},
};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::watch,
    time::timeout,
};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

const KEEPALIVE: Duration = Duration::from_secs(1);

struct Pki {
    root: Certificate,
    server: (Certificate, KeyPair),
    client: (Certificate, KeyPair),
}

fn pki() -> Pki {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec![]).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let root = params.self_signed(&key).unwrap();
    let issuer = Issuer::new(params, key);
    let leaf = |usage| {
        let key = KeyPair::generate().unwrap();
        let mut params =
            CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
        params.extended_key_usages = vec![usage];
        (params.signed_by(&key, &issuer).unwrap(), key)
    };
    Pki {
        server: leaf(ExtendedKeyUsagePurpose::ServerAuth),
        client: leaf(ExtendedKeyUsagePurpose::ClientAuth),
        root,
    }
}

impl Pki {
    fn client_tls(&self) -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(self.root.der().clone()).unwrap();
        let mut config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_protocol_versions(&[&rustls::version::TLS13])
                .unwrap()
                .with_root_certificates(roots)
                .with_client_auth_cert(
                    vec![self.client.0.der().clone()],
                    PrivatePkcs8KeyDer::from(self.client.1.serialize_der()).into(),
                )
                .unwrap();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Arc::new(config)
    }

    fn serve(&self, supported: VersionRange) -> (SocketAddr, Handle<SocketAddr>) {
        let resolver = pem_resolver(
            self.server.0.pem().as_bytes(),
            self.server.1.serialize_pem().as_bytes(),
        )
        .unwrap();
        let config = server_config(resolver, Some(self.root.pem().as_bytes()), None).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let handle = Handle::new();
        let protocol = AgentProtocol {
            supported,
            keepalive: KEEPALIVE,
            handshake_timeout: Duration::from_secs(2),
        };
        tokio::spawn(
            axum_server::from_tcp_rustls(listener, RustlsConfig::from_config(config))
                .unwrap()
                .handle(handle.clone())
                .serve(agent_router_with(protocol).into_make_service()),
        );
        (address, handle)
    }
}

fn range(min: u32, max: u32) -> VersionRange {
    VersionRange::new(min, max).unwrap()
}

struct Agent {
    status: watch::Receiver<Status>,
    stop: watch::Sender<bool>,
}

fn agent(pki: &Pki, port: u16, supported: VersionRange) -> Agent {
    let settings = Settings {
        supported,
        handshake_timeout: Duration::from_secs(2),
        backoff: BackoffPolicy {
            base: Duration::from_millis(50),
            cap: Duration::from_millis(400),
        },
        ..Settings::new("127.0.0.1".into(), port, pki.client_tls(), "test")
    };
    let (status_tx, status) = watch::channel(Status::Connecting { attempt: 0 });
    let (stop, stop_rx) = watch::channel(false);
    tokio::spawn(run(settings, status_tx, stop_rx));
    Agent { status, stop }
}

impl Agent {
    async fn reach(
        &mut self,
        what: &str,
        limit: Duration,
        f: impl FnMut(&Status) -> bool,
    ) -> Status {
        let outcome = timeout(limit, self.status.wait_for(f))
            .await
            .map(|result| result.map(|status| status.clone()));
        match outcome {
            Ok(Ok(status)) => status,
            Ok(Err(_)) => panic!("agent status channel closed while waiting for {what}"),
            Err(_) => panic!(
                "timed out waiting for {what}; last {:?}",
                *self.status.borrow()
            ),
        }
    }

    async fn connected(&mut self) -> u32 {
        match self
            .reach("connection", Duration::from_secs(10), |s| {
                matches!(s, Status::Connected { .. })
            })
            .await
        {
            Status::Connected { version, keepalive } => {
                assert_eq!(keepalive, KEEPALIVE);
                version
            }
            _ => unreachable!(),
        }
    }

    async fn stays_connected(&mut self, period: Duration) {
        let dropped = timeout(
            period,
            self.status
                .wait_for(|s| !matches!(s, Status::Connected { .. })),
        )
        .await
        .is_ok();
        assert!(!dropped, "session dropped: {:?}", *self.status.borrow());
    }

    async fn stop(mut self) {
        self.stop.send_replace(true);
        self.reach("stop", Duration::from_secs(5), |s| *s == Status::Stopped)
            .await;
    }
}

#[tokio::test]
async fn different_supported_versions_interoperate() {
    let pki = pki();
    let cases = [
        (range(1, 2), range(1, 1), 1),
        (range(1, 2), range(2, 3), 2),
        (range(2, 3), range(1, 2), 2),
    ];
    let runs = cases.map(|(server, client, expected)| {
        let (address, _handle) = pki.serve(server);
        let mut agent = agent(&pki, address.port(), client);
        async move {
            assert_eq!(agent.connected().await, expected);
            // Keepalive pings go unanswered only if messages stop flowing.
            agent.stays_connected(KEEPALIVE * 3).await;
            agent.stop().await;
        }
    });
    futures_util::future::join_all(runs).await;
}

#[tokio::test]
async fn incompatible_versions_are_refused_with_the_server_range() {
    let pki = pki();
    let (address, _handle) = pki.serve(range(2, 3));
    let mut agent = agent(&pki, address.port(), range(1, 1));
    let status = agent
        .reach("refusal", Duration::from_secs(10), |s| {
            matches!(s, Status::Waiting { .. })
        })
        .await;
    let Status::Waiting { retry_in, reason } = status else {
        unreachable!()
    };
    assert_eq!(
        reason,
        Reason::Incompatible {
            server: range(2, 3)
        }
    );
    assert!(retry_in >= Duration::from_millis(200), "{retry_in:?}");
    agent.stop().await;
}

type Raw = tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>;

async fn raw(pki: &Pki, port: u16) -> Raw {
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let tls = tokio_rustls::TlsConnector::from(pki.client_tls())
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .unwrap();
    let request = format!("wss://localhost:{port}/agent/connect")
        .into_client_request()
        .unwrap();
    tokio_tungstenite::client_async(request, tls)
        .await
        .unwrap()
        .0
}

async fn next_body(socket: &mut Raw) -> Option<Body> {
    while let Some(Ok(message)) = socket.next().await {
        if let Message::Binary(bytes) = message {
            return midas_protocol::decode(&bytes).unwrap();
        }
        if let Message::Close(_) = message {
            return None;
        }
    }
    None
}

async fn closes(socket: &mut Raw, limit: Duration) -> bool {
    timeout(limit, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn invalid_hellos_are_refused_and_closed() {
    let pki = pki();
    let (address, _handle) = pki.serve(range(1, 1));
    let invalid = [
        Body::Ping(Ping { nonce: 1 }),
        Body::Hello(Hello {
            min_version: 0,
            max_version: 1,
            agent_version: "test".into(),
            platform: "test".into(),
        }),
        Body::Hello(Hello {
            min_version: 1,
            max_version: 1,
            agent_version: "x".repeat(65),
            platform: "test".into(),
        }),
    ];
    for first in invalid {
        let mut socket = raw(&pki, address.port()).await;
        socket
            .send(Message::Binary(midas_protocol::encode(first).into()))
            .await
            .unwrap();
        let Some(Body::Reject(reject)) = next_body(&mut socket).await else {
            panic!("expected a refusal");
        };
        assert_eq!(reject.reason, RejectReason::InvalidHello as i32);
        assert_eq!((reject.min_version, reject.max_version), (1, 1));
        assert!(closes(&mut socket, Duration::from_secs(3)).await);
    }
}

#[tokio::test]
async fn silent_sessions_are_dropped_after_three_keepalives() {
    let pki = pki();
    let (address, _handle) = pki.serve(range(1, 1));
    let mut socket = raw(&pki, address.port()).await;
    let hello = Body::Hello(Hello {
        min_version: 1,
        max_version: 1,
        agent_version: "test".into(),
        platform: "test".into(),
    });
    socket
        .send(Message::Binary(midas_protocol::encode(hello).into()))
        .await
        .unwrap();
    let Some(Body::Welcome(welcome)) = next_body(&mut socket).await else {
        panic!("expected a welcome");
    };
    assert_eq!((welcome.version, welcome.keepalive_seconds), (1, 1));
    let started = std::time::Instant::now();
    assert!(closes(&mut socket, KEEPALIVE * 6).await);
    assert!(
        started.elapsed() >= KEEPALIVE * 2,
        "{:?}",
        started.elapsed()
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    Forward,
    Cut,
    Silent,
}

async fn proxy(upstream: SocketAddr) -> (u16, Arc<watch::Sender<Link>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let link = Arc::new(watch::channel(Link::Forward).0);
    let modes = link.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut inbound, _)) = listener.accept().await else {
                return;
            };
            let mut mode = modes.subscribe();
            if *mode.borrow_and_update() == Link::Cut {
                continue;
            }
            tokio::spawn(async move {
                let Ok(mut outbound) = TcpStream::connect(upstream).await else {
                    return;
                };
                if *mode.borrow_and_update() == Link::Forward {
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => return,
                        _ = mode.changed() => {}
                    }
                }
                if *mode.borrow_and_update() == Link::Silent {
                    let _ = mode.changed().await;
                }
            });
        }
    });
    (port, link)
}

#[tokio::test]
async fn agent_reconnects_after_connection_loss() {
    let pki = pki();
    let (address, _handle) = pki.serve(range(1, 1));
    let (port, link) = proxy(address).await;
    let mut agent = agent(&pki, port, range(1, 1));
    agent.connected().await;

    link.send_replace(Link::Cut);
    agent
        .reach("loss after cut", Duration::from_secs(5), |s| {
            !matches!(s, Status::Connected { .. })
        })
        .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!matches!(*agent.status.borrow(), Status::Connected { .. }));
    link.send_replace(Link::Forward);
    agent.connected().await;
    agent.stays_connected(KEEPALIVE * 2).await;

    link.send_replace(Link::Silent);
    let started = std::time::Instant::now();
    let status = agent
        .reach("loss after silence", KEEPALIVE * 4, |s| {
            !matches!(s, Status::Connected { .. })
        })
        .await;
    assert!(
        matches!(
            status,
            Status::Waiting {
                reason: Reason::Lost,
                ..
            }
        ),
        "{status:?} after {:?}",
        started.elapsed()
    );
    link.send_replace(Link::Forward);
    agent.connected().await;
    agent.stop().await;
}

#[tokio::test]
async fn stopping_is_prompt_while_the_server_stays_silent() {
    let pki = pki();
    let resolver = pem_resolver(
        pki.server.0.pem().as_bytes(),
        pki.server.1.serialize_pem().as_bytes(),
    )
    .unwrap();
    let config = server_config(resolver, Some(pki.root.pem().as_bytes()), None).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mute = axum::Router::new().route(
        "/agent/connect",
        axum::routing::get(|upgrade: axum::extract::ws::WebSocketUpgrade| async {
            upgrade.on_upgrade(|socket| async move {
                tokio::time::sleep(Duration::from_secs(60)).await;
                drop(socket);
            })
        }),
    );
    tokio::spawn(
        axum_server::from_tcp_rustls(listener, RustlsConfig::from_config(config))
            .unwrap()
            .serve(mute.into_make_service()),
    );
    let settings = Settings {
        handshake_timeout: Duration::from_secs(30),
        ..Settings::new("127.0.0.1".into(), port, pki.client_tls(), "test")
    };
    let (status_tx, status) = watch::channel(Status::Connecting { attempt: 0 });
    let (stop, stop_rx) = watch::channel(false);
    let task = tokio::spawn(run(settings, status_tx, stop_rx));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(*status.borrow(), Status::Connecting { attempt: 1 });
    let started = std::time::Instant::now();
    stop.send_replace(true);
    timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(*status.borrow(), Status::Stopped);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
}
