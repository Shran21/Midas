use std::{sync::Arc, time::Duration};

use axum::http::StatusCode;
use axum_server::tls_rustls::RustlsConfig;
use midas_agent_core::{
    backoff::BackoffPolicy,
    connection::{Reason, Settings, Status, run},
    enrollment::{EnrollError, Identity, Server, enroll, poll},
};
use midas_protocol::enrollment::{EnrollAnswer, EnrollRequest, Kind, Platform, PollAnswer};
use midas_server::{
    agent::AgentProtocol,
    agent_router_with,
    audit::Actor,
    auth::{
        AuthConfig,
        http::{WebAuth, router},
        sessions::SessionConfig,
    },
    authorization::{Authorizer, BuiltinRole, Scope},
    database::Database,
    enrollment::{Authority, Enrollment},
    transport::{PeerCertificates, pem_resolver, server_config},
    web_router_with,
};
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair};
use rustls::{ClientConfig, RootCertStore};
use serde_json::json;
use sqlx::PgConnection;
use tokio::{sync::watch, time::timeout};
use uuid::Uuid;
use webauthn_rs::prelude::Url;
use zeroize::Zeroizing;

use super::administration::{Client, ORIGIN, call, enroll as operator, passkey};

fn authority(name: &str) -> (rcgen::Certificate, Issuer<'static, KeyPair>, String) {
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, name);
    let certificate = params.self_signed(&key).unwrap();
    let pem = key.serialize_pem();
    (certificate, Issuer::new(params, key), pem)
}

fn roots(certificate: &rcgen::Certificate) -> RootCertStore {
    let mut roots = RootCertStore::empty();
    roots.add(certificate.der().clone()).unwrap();
    roots
}

fn verifying(certificate: &rcgen::Certificate) -> Arc<ClientConfig> {
    Arc::new(
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_root_certificates(roots(certificate))
            .with_no_client_auth(),
    )
}

fn listener() -> (std::net::TcpListener, u16) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

struct Agent {
    status: watch::Receiver<Status>,
    stop: watch::Sender<bool>,
}

impl Agent {
    fn start(port: u16, tls: Arc<ClientConfig>) -> Self {
        Self::metered(port, tls, None)
    }

    fn metered(
        port: u16,
        tls: Arc<ClientConfig>,
        metrics: Option<midas_agent_core::metrics::Shared>,
    ) -> Self {
        Self::with(port, tls, metrics, None)
    }

    fn with(
        port: u16,
        tls: Arc<ClientConfig>,
        metrics: Option<midas_agent_core::metrics::Shared>,
        inventory: Option<(
            midas_agent_core::inventory::Shared,
            midas_agent_core::inventory::Collector,
        )>,
    ) -> Self {
        let settings = Settings {
            metrics,
            inventory: inventory.as_ref().map(|(shared, _)| shared.clone()),
            handshake_timeout: Duration::from_secs(3),
            backoff: BackoffPolicy {
                base: Duration::from_millis(50),
                cap: Duration::from_millis(300),
            },
            ..Settings::new("127.0.0.1".into(), port, tls, "windows")
        };
        let (status_tx, status) = watch::channel(Status::Connecting { attempt: 0 });
        let (stop, stop_rx) = watch::channel(false);
        if let Some((shared, collector)) = inventory {
            tokio::spawn(midas_agent_core::inventory::schedule(
                collector,
                shared,
                Duration::ZERO,
                stop_rx.clone(),
            ));
        }
        tokio::spawn(run(settings, status_tx, stop_rx));
        Self { status, stop }
    }

    fn checked(
        port: u16,
        tls: Arc<ClientConfig>,
        health: midas_agent_core::health::Shared,
        checker: midas_agent_core::health::Checker,
        capabilities: Vec<String>,
    ) -> Self {
        let settings = Settings {
            health: Some(health.clone()),
            capabilities,
            handshake_timeout: Duration::from_secs(3),
            ..Settings::new("127.0.0.1".into(), port, tls, "windows")
        };
        let (status_tx, status) = watch::channel(Status::Connecting { attempt: 0 });
        let (stop, stop_rx) = watch::channel(false);
        tokio::spawn(midas_agent_core::health::schedule(
            checker,
            health,
            Duration::ZERO,
            stop_rx.clone(),
        ));
        tokio::spawn(run(settings, status_tx, stop_rx));
        Self { status, stop }
    }

    async fn reach(&mut self, what: &str, f: impl FnMut(&Status) -> bool) -> Status {
        let outcome = timeout(Duration::from_secs(10), self.status.wait_for(f))
            .await
            .map(|result| result.map(|status| status.clone()));
        match outcome {
            Ok(Ok(status)) => status,
            _ => panic!(
                "timed out waiting for {what}; last {:?}",
                *self.status.borrow()
            ),
        }
    }

    async fn never_connects(&mut self, period: Duration) {
        let connected = timeout(
            period,
            self.status
                .wait_for(|s| matches!(s, Status::Connected { .. })),
        )
        .await
        .is_ok();
        assert!(!connected, "agent connected unexpectedly");
    }
}

fn software(count: usize) -> midas_agent_core::inventory::Snapshot {
    let mut snapshot = midas_agent_core::inventory::Snapshot::new();
    midas_agent_core::inventory::insert(&mut snapshot, "os", "name", "Test OS");
    for n in 0..count {
        midas_agent_core::inventory::insert(
            &mut snapshot,
            "software",
            &format!("package-{n:04}"),
            "1.0",
        );
    }
    snapshot
}

/// A minimal SMTP server that accepts every message, enough for the real client without TLS.
async fn mail_sink() -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let store = received.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let store = store.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                let _ = write.write_all(b"220 sink ESMTP\r\n").await;
                let mut data: Option<String> = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(body) = data.as_mut() {
                        if line == "." {
                            store.lock().unwrap().push(std::mem::take(body));
                            data = None;
                            let _ = write.write_all(b"250 queued\r\n").await;
                        } else {
                            body.push_str(&line);
                            body.push('\n');
                        }
                        continue;
                    }
                    let command = line.to_ascii_uppercase();
                    let reply: &[u8] = if command.starts_with("DATA") {
                        data = Some(String::new());
                        b"354 go ahead\r\n"
                    } else if command.starts_with("QUIT") {
                        let _ = write.write_all(b"221 bye\r\n").await;
                        break;
                    } else {
                        b"250 ok\r\n"
                    };
                    let _ = write.write_all(reply).await;
                }
            });
        }
    });
    (port, received)
}

type HealthRow = (String, String, String, Vec<String>, f64);

/// Waits until the stored check results satisfy `ready`.
async fn health_rows(
    admin: &mut PgConnection,
    device: Uuid,
    mut ready: impl FnMut(&[HealthRow]) -> bool,
) -> Vec<HealthRow> {
    for _ in 0..150 {
        let rows: Vec<HealthRow> = sqlx::query_as("SELECT check_id, state, code, subjects, extract(epoch FROM changed_at)::float8 FROM midas.device_health WHERE device_id = $1 ORDER BY check_id")
            .bind(device).fetch_all(&mut *admin).await.unwrap();
        if ready(&rows) {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("health results never reached the expected state");
}

fn health_check(
    id: &str,
    state: midas_protocol::HealthState,
    code: &str,
    subjects: &[&str],
) -> midas_protocol::HealthCheck {
    midas_agent_core::health::result(
        id,
        state,
        code,
        &format!("{id} is {code}"),
        &subjects
            .iter()
            .map(|subject| (*subject).to_owned())
            .collect::<Vec<_>>(),
    )
}

/// Waits until the stored inventory satisfies `ready`; returns generation and reconciliation time.
async fn inventory_state(
    admin: &mut PgConnection,
    device: Uuid,
    mut ready: impl FnMut(&std::collections::HashMap<String, String>) -> bool,
) -> (Option<i64>, Option<f64>) {
    for _ in 0..150 {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key, value FROM midas.device_inventory WHERE device_id = $1")
                .bind(device)
                .fetch_all(&mut *admin)
                .await
                .unwrap();
        let items: std::collections::HashMap<_, _> = rows.into_iter().collect();
        if ready(&items) {
            return sqlx::query_as("SELECT inventory_generation, extract(epoch FROM inventory_reconciled_at)::float8 FROM midas.devices WHERE id = $1")
                .bind(device)
                .fetch_one(&mut *admin)
                .await
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("inventory never reached the expected state");
}

fn sample(at: i64) -> midas_protocol::Sample {
    midas_protocol::Sample {
        at,
        cpu_permille: 250,
        memory_used: 4 << 30,
        memory_total: 8 << 30,
        disk_used: 100 << 30,
        disk_total: 200 << 30,
        net_rx: 1000,
        net_tx: 2000,
    }
}

async fn stored(admin: &mut PgConnection, device: Uuid, expected: i64) {
    let mut count = 0;
    for _ in 0..100 {
        count =
            sqlx::query_scalar("SELECT count(*) FROM midas.device_metrics WHERE device_id = $1")
                .bind(device)
                .fetch_one(&mut *admin)
                .await
                .unwrap();
        if count == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("stored {count} samples, expected {expected}");
}

async fn online(admin: &mut PgConnection, device: Uuid) -> bool {
    sqlx::query_scalar("SELECT online FROM midas.devices WHERE id = $1")
        .bind(device)
        .fetch_one(&mut *admin)
        .await
        .unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Link {
    Forward,
    Silent,
}

/// Forwards to `upstream`; switching to `Silent` keeps sockets open but stops all traffic.
async fn proxy(upstream: std::net::SocketAddr) -> (u16, Arc<watch::Sender<Link>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let link = Arc::new(watch::channel(Link::Forward).0);
    let modes = link.clone();
    tokio::spawn(async move {
        while let Ok((mut inbound, _)) = listener.accept().await {
            let mut mode = modes.subscribe();
            tokio::spawn(async move {
                let Ok(mut outbound) = tokio::net::TcpStream::connect(upstream).await else {
                    return;
                };
                if *mode.borrow_and_update() == Link::Forward {
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound) => return,
                        _ = mode.changed() => {}
                    }
                }
                let _ = mode.changed().await;
            });
        }
    });
    (port, link)
}

async fn admin_of(web: &Arc<WebAuth>, policy: &Authorizer, tenant: Uuid, login: &str) -> Client {
    let (operator_id, mut key) = operator(web, login, login).await;
    policy
        .set_membership(tenant, operator_id, true, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .assign_role(
            tenant,
            operator_id,
            BuiltinRole::Administrator.id(),
            Scope::Tenant,
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    passkey(&router(web.clone()), login, &mut key).await
}

pub async fn verify(database: &Database, admin: &mut PgConnection) {
    let (tenant, other) = (Uuid::new_v4(), Uuid::new_v4());
    let mut site = Uuid::nil();
    for (id, name) in [(tenant, "Enrollment A"), (other, "Enrollment B")] {
        let mut tx = database.begin(id).await.unwrap();
        tx.create_tenant(name).await.unwrap();
        let created = tx.create_site(name).await.unwrap().id;
        if id == tenant {
            site = created;
        }
        tx.commit().await.unwrap();
    }

    let (agent_ca, agent_issuer, agent_key) = authority("MIDAS agent CA");
    let (server_ca, server_issuer, _) = authority("MIDAS test server CA");
    let server_key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into()]).unwrap();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server_certificate = params.signed_by(&server_key, &server_issuer).unwrap();
    let resolver = || {
        pem_resolver(
            server_certificate.pem().as_bytes(),
            server_key.serialize_pem().as_bytes(),
        )
        .unwrap()
    };

    let enrollment = Arc::new(Enrollment::new(
        database,
        Authority::from_pem(&agent_ca.pem(), &agent_key).unwrap(),
    ));
    let mut key = [0; 32];
    getrandom::fill(&mut key).unwrap();
    let mut config = AuthConfig::new(Url::parse(ORIGIN).unwrap(), Zeroizing::new(key));
    config.account_attempts = 300;
    config.source_attempts = 1000;
    let web = Arc::new(
        WebAuth::new(database, config, SessionConfig::default(), 4)
            .await
            .unwrap()
            .with_enrollment(enrollment.clone()),
    );
    let app = router(web.clone());
    let policy = Authorizer::new(database);
    let manager = admin_of(&web, &policy, tenant, "enrollment-manager").await;
    let outsider = admin_of(&web, &policy, other, "enrollment-outsider").await;

    let (web_listener, web_port) = listener();
    tokio::spawn(
        axum_server::from_tcp_rustls(
            web_listener,
            RustlsConfig::from_config(server_config(resolver(), None, None).unwrap()),
        )
        .unwrap()
        .serve(web_router_with(Some(enrollment.clone())).into_make_service()),
    );
    let (agent_listener, agent_port) = listener();
    let protocol = AgentProtocol {
        keepalive: Duration::from_secs(1),
        identities: Some(enrollment.clone()),
        ..AgentProtocol::default()
    };
    tokio::spawn(
        axum_server::from_tcp(agent_listener)
            .unwrap()
            .acceptor(PeerCertificates::new(
                server_config(resolver(), Some(agent_ca.pem().as_bytes()), None).unwrap(),
            ))
            .serve(agent_router_with(protocol).into_make_service()),
    );
    let server = Server {
        host: "127.0.0.1".into(),
        port: web_port,
        tls: verifying(&server_ca),
        timeout: Duration::from_secs(5),
    };

    let create = |client: &Client, tenant: Uuid, body: serde_json::Value| {
        let (app, client) = (app.clone(), client.clone());
        async move {
            call(
                &app,
                "POST",
                &format!("/api/tenants/{tenant}/enrollment-tokens"),
                Some(&client),
                Some(body),
                &[],
            )
            .await
        }
    };
    let spec = json!({ "site_id": site, "platform": "windows", "kind": "workstation", "preapproved": false, "lifetime_hours": 24 });
    let (status, _, _) = create(&outsider, tenant, spec.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = create(
        &manager,
        tenant,
        json!({ "site_id": site, "platform": "windows", "kind": "workstation", "preapproved": false, "lifetime_hours": 0 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, created, _) = create(&manager, tenant, spec.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let token = created["token"].as_str().unwrap().to_owned();
    assert!(token.starts_with("midas-enroll-"));

    let identity = Identity::generate().unwrap();
    let request = |token: &str, platform, kind| EnrollRequest {
        token: token.into(),
        csr: identity.signing_request().unwrap(),
        platform,
        kind,
        name: "PC-01".into(),
    };
    for (platform, kind) in [
        (Platform::Linux, Kind::Workstation),
        (Platform::Windows, Kind::Server),
    ] {
        assert_eq!(
            enroll(&server, &request(&token, platform, kind)).await,
            Err(EnrollError::Rejected),
            "wrong target must be refused"
        );
    }
    let bad_csr = EnrollRequest {
        csr: "-----BEGIN CERTIFICATE REQUEST-----\nAAAA\n-----END CERTIFICATE REQUEST-----\n"
            .into(),
        ..request(&token, Platform::Windows, Kind::Workstation)
    };
    assert_eq!(enroll(&server, &bad_csr).await, Err(EnrollError::Invalid));
    let EnrollAnswer::Pending {
        request_id,
        request_secret,
    } = enroll(
        &server,
        &request(&token, Platform::Windows, Kind::Workstation),
    )
    .await
    .unwrap()
    else {
        panic!("a reviewed token must wait for approval");
    };
    assert_eq!(
        enroll(
            &server,
            &request(&token, Platform::Windows, Kind::Workstation)
        )
        .await,
        Err(EnrollError::Rejected),
        "a used token must be refused"
    );
    let audit_rows = "SELECT count(*) FROM midas.platform_audit";
    let before: i64 = sqlx::query_scalar(audit_rows)
        .fetch_one(&mut *admin)
        .await
        .unwrap();
    for token in [format!("midas-enroll-{}", "0".repeat(64)), "guess".into()] {
        assert_eq!(
            enroll(
                &server,
                &request(&token, Platform::Windows, Kind::Workstation)
            )
            .await,
            Err(EnrollError::Rejected)
        );
    }
    let after: i64 = sqlx::query_scalar(audit_rows)
        .fetch_one(&mut *admin)
        .await
        .unwrap();
    assert_eq!(
        after, before,
        "unauthenticated refusals must not grow the audit trail"
    );
    assert_eq!(
        poll(&server, request_id, &"0".repeat(64)).await,
        Err(EnrollError::Rejected)
    );
    assert_eq!(
        poll(&server, request_id, &request_secret).await,
        Ok(PollAnswer::Pending)
    );

    let (status, pending, _) = call(
        &app,
        "GET",
        &format!("/api/tenants/{tenant}/devices/pending"),
        Some(&manager),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pending.as_array().unwrap().len(), 1, "{pending}");
    assert_eq!(pending[0]["name"], "PC-01");
    assert_eq!(pending[0]["site_id"], site.to_string());
    let device: Uuid = pending[0]["id"].as_str().unwrap().parse().unwrap();
    for (client, path) in [
        (&outsider, format!("/api/tenants/{tenant}/devices/pending")),
        (
            &outsider,
            format!("/api/tenants/{tenant}/devices/{device}/approve"),
        ),
    ] {
        let method = if path.ends_with("pending") {
            "GET"
        } else {
            "POST"
        };
        let (status, _, _) = call(&app, method, &path, Some(client), None, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    let (status, own, _) = call(
        &app,
        "GET",
        &format!("/api/tenants/{other}/devices/pending"),
        Some(&outsider),
        None,
        &[],
    )
    .await;
    assert_eq!((status, own), (StatusCode::OK, json!([])));
    let decide = |decision: &str, device: Uuid| {
        let (app, client, path) = (
            app.clone(),
            manager.clone(),
            format!("/api/tenants/{tenant}/devices/{device}/{decision}"),
        );
        async move { call(&app, "POST", &path, Some(&client), None, &[]).await.0 }
    };
    assert_eq!(decide("approve", device).await, StatusCode::NO_CONTENT);
    assert_eq!(decide("approve", device).await, StatusCode::NOT_FOUND);
    let PollAnswer::Issued(issued) = poll(&server, request_id, &request_secret).await.unwrap()
    else {
        panic!("approved request must return its certificate");
    };
    assert_eq!(issued.device_id, device);

    let tls = identity
        .client_config(roots(&server_ca), &issued.certificate)
        .unwrap();
    let mut agent = Agent::start(agent_port, tls);
    agent
        .reach("enrolled connection", |s| {
            matches!(s, Status::Connected { .. })
        })
        .await;

    let stranger_key = KeyPair::generate().unwrap();
    let mut stranger = CertificateParams::new(vec![]).unwrap();
    stranger.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let stranger = stranger.signed_by(&stranger_key, &agent_issuer).unwrap();
    let stranger_identity = Identity::from_pem(&stranger_key.serialize_pem()).unwrap();
    let mut unregistered = Agent::start(
        agent_port,
        stranger_identity
            .client_config(roots(&server_ca), &stranger.pem())
            .unwrap(),
    );
    unregistered
        .reach("refusal of an unregistered certificate", |s| {
            matches!(
                s,
                Status::Waiting {
                    reason: Reason::Handshake,
                    ..
                }
            )
        })
        .await;
    unregistered.never_connects(Duration::from_secs(1)).await;
    unregistered.stop.send_replace(true);

    assert_eq!(decide("revoke", device).await, StatusCode::NO_CONTENT);
    agent
        .reach("closure after revocation", |s| {
            matches!(
                s,
                Status::Waiting {
                    reason: Reason::Lost,
                    ..
                }
            )
        })
        .await;
    agent.never_connects(Duration::from_secs(2)).await;
    agent.stop.send_replace(true);
    assert_eq!(
        poll(&server, request_id, &request_secret).await,
        Err(EnrollError::Rejected)
    );

    let (_, created, _) = create(&manager, tenant, spec.clone()).await;
    let rejected_token = created["token"].as_str().unwrap().to_owned();
    let EnrollAnswer::Pending {
        request_id: rejected_id,
        request_secret: rejected_secret,
    } = enroll(
        &server,
        &request(&rejected_token, Platform::Windows, Kind::Workstation),
    )
    .await
    .unwrap()
    else {
        panic!("expected a pending request");
    };
    let (_, pending, _) = call(
        &app,
        "GET",
        &format!("/api/tenants/{tenant}/devices/pending"),
        Some(&manager),
        None,
        &[],
    )
    .await;
    let rejected_device: Uuid = pending[0]["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE midas.enrollment_requests SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1")
        .bind(rejected_id)
        .execute(&mut *admin)
        .await
        .unwrap();
    let (_, hidden, _) = call(
        &app,
        "GET",
        &format!("/api/tenants/{tenant}/devices/pending"),
        Some(&manager),
        None,
        &[],
    )
    .await;
    assert_eq!(hidden, json!([]), "expired requests cannot be approved");
    assert_eq!(
        decide("approve", rejected_device).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        poll(&server, rejected_id, &rejected_secret).await,
        Err(EnrollError::Rejected)
    );
    sqlx::query("UPDATE midas.enrollment_requests SET expires_at = clock_timestamp() + interval '1 day' WHERE id = $1")
        .bind(rejected_id)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert_eq!(
        decide("reject", rejected_device).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        poll(&server, rejected_id, &rejected_secret).await,
        Err(EnrollError::Rejected)
    );

    let (_, created, _) = create(
        &manager,
        tenant,
        json!({ "site_id": null, "platform": "linux", "kind": "server", "preapproved": true, "lifetime_hours": 1 }),
    )
    .await;
    let deployment = Identity::generate().unwrap();
    let answer = enroll(
        &server,
        &EnrollRequest {
            token: created["token"].as_str().unwrap().into(),
            csr: deployment.signing_request().unwrap(),
            platform: Platform::Linux,
            kind: Kind::Server,
            name: "srv-01".into(),
        },
    )
    .await
    .unwrap();
    let EnrollAnswer::Issued(preapproved) = answer else {
        panic!("a preapproved token must issue at once");
    };
    let mut server_agent = Agent::start(
        agent_port,
        deployment
            .client_config(roots(&server_ca), &preapproved.certificate)
            .unwrap(),
    );
    server_agent
        .reach("preapproved connection", |s| {
            matches!(s, Status::Connected { .. })
        })
        .await;

    let connector_path = format!("/api/tenants/{tenant}/directory-connector");
    let connector = |device: Uuid, url: &str, bases: &[&str]| {
        json!({
            "device_id": device,
            "domain": "corp.example",
            "url": url,
            "ca_pem": server_ca.pem(),
            "base_dns": bases,
        })
    };
    let workstations = ["OU=Workstations,DC=corp,DC=example"];
    let put = |client: &Client, body: serde_json::Value| {
        let (app, client, path) = (app.clone(), client.clone(), connector_path.clone());
        async move { call(&app, "PUT", &path, Some(&client), Some(body), &[]).await }
    };
    let server_device = preapproved.device_id;
    let (status, _, _) = put(
        &outsider,
        connector(server_device, "ldaps://dc.corp.example", &workstations),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    for invalid in [
        connector(Uuid::new_v4(), "ldaps://dc.corp.example", &workstations),
        connector(device, "ldaps://dc.corp.example", &workstations),
        connector(server_device, "ldap://dc.corp.example", &workstations),
        connector(
            server_device,
            "ldaps://dc.corp.example",
            &["OU=X,DC=other,DC=example"],
        ),
        connector(server_device, "ldaps://dc.corp.example", &[]),
    ] {
        let (status, body, _) = put(&manager, invalid).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let (status, _, _) = call(&app, "GET", &connector_path, Some(&manager), None, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no connector designated yet");
    let check = |expected: i64| {
        let (app, client, path) = (app.clone(), manager.clone(), connector_path.clone());
        async move {
            for _ in 0..50 {
                let (status, view, _) = call(&app, "GET", &path, Some(&client), None, &[]).await;
                assert_eq!(status, StatusCode::OK, "{view}");
                if view["last_check"]["revision"] == expected {
                    return view;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("connector report for revision {expected} did not arrive");
        }
    };
    for revision in [1, 2] {
        let (status, body, _) = put(
            &manager,
            connector(server_device, "ldaps://dc.corp.example:636", &workstations),
        )
        .await;
        assert_eq!(
            (status, body["revision"].as_i64()),
            (StatusCode::OK, Some(revision))
        );
        let view = check(revision).await;
        assert_eq!(view["device_id"], server_device.to_string());
        assert_eq!(view["base_dns"], json!(workstations));
        assert_eq!(view["last_check"]["ok"], false);
        assert_eq!(view["last_check"]["error"], "unconfigured");
        assert!(view.get("ca_pem").is_none());
    }
    let (status, _, _) = call(&app, "GET", &connector_path, Some(&outsider), None, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = call(
        &app,
        "GET",
        &format!("/api/tenants/{other}/directory-connector"),
        Some(&outsider),
        None,
        &[],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the other tenant has no connector"
    );
    server_agent.stop.send_replace(true);

    // Presence and metrics over a link the test can cut.
    let (proxy_port, link) = proxy(([127, 0, 0, 1], agent_port).into()).await;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let minute = now - now.rem_euclid(60);
    let buffer = midas_agent_core::metrics::Buffer::shared(midas_agent_core::metrics::CAPACITY);
    for n in (1..=130).rev() {
        midas_agent_core::metrics::lock(&buffer).push(sample(minute - n * 60));
    }
    let mut metered = Agent::metered(
        proxy_port,
        deployment
            .client_config(roots(&server_ca), &preapproved.certificate)
            .unwrap(),
        Some(buffer.clone()),
    );
    metered
        .reach("metered connection", |s| {
            matches!(s, Status::Connected { .. })
        })
        .await;
    stored(admin, server_device, 130).await;
    assert!(online(admin, server_device).await);
    assert!(midas_agent_core::metrics::lock(&buffer).is_empty());

    link.send_replace(Link::Silent);
    metered
        .reach("loss of the metered link", |s| {
            matches!(s, Status::Waiting { .. })
        })
        .await;
    tokio::time::sleep(Duration::from_millis(3500)).await;
    let offline = enrollment.sweep(Duration::from_secs(3)).await.unwrap();
    assert!(
        offline
            .iter()
            .any(|identity| identity.device == server_device)
    );
    assert!(
        !online(admin, server_device).await,
        "offline after three missed keepalives"
    );
    for n in 0..5 {
        midas_agent_core::metrics::lock(&buffer).push(sample(minute + n * 60));
    }
    link.send_replace(Link::Forward);
    metered
        .reach("reconnection", |s| matches!(s, Status::Connected { .. }))
        .await;
    stored(admin, server_device, 135).await;
    assert!(online(admin, server_device).await);
    let changes: Vec<bool> = sqlx::query_scalar(
        "SELECT online FROM midas.device_connectivity WHERE device_id = $1 ORDER BY at",
    )
    .bind(server_device)
    .fetch_all(&mut *admin)
    .await
    .unwrap();
    assert!(changes.ends_with(&[false, true]), "{changes:?}");
    metered.stop.send_replace(true);

    // Rollup and expiry: minute samples go only after their hour is rolled up.
    let old = now - 40 * 86_400;
    let old_hour = old - old.rem_euclid(3600);
    sqlx::query("INSERT INTO midas.device_metrics (tenant_id, device_id, at, cpu_permille, memory_used, memory_total, disk_used, disk_total, net_rx, net_tx) VALUES ($1, $2, to_timestamp($3), 100, 1, 2, 1, 2, 10, 20), ($1, $2, to_timestamp($3 + 60), 300, 1, 2, 1, 2, 30, 40)")
        .bind(tenant).bind(server_device).bind(old_hour as f64).execute(&mut *admin).await.unwrap();
    sqlx::query("INSERT INTO midas.device_metrics_hourly (tenant_id, device_id, hour, samples, cpu_avg_permille, cpu_max_permille, memory_used_avg, memory_used_max, memory_total, disk_used_max, disk_total, net_rx, net_tx) VALUES ($1, $2, date_trunc('hour', clock_timestamp() - interval '14 months'), 1, 0, 0, 0, 0, 0, 0, 0, 0, 0)")
        .bind(tenant).bind(server_device).execute(&mut *admin).await.unwrap();
    let (minutes, hours) = enrollment.expire(30, 13).await.unwrap();
    assert_eq!(
        (minutes, hours),
        (0, 1),
        "unrolled minute samples must stay"
    );
    assert!(enrollment.rollup().await.unwrap() > 0);
    let rolled: (i32, i16, i16, i64, i64) = sqlx::query_as("SELECT samples, cpu_avg_permille, cpu_max_permille, net_rx, net_tx FROM midas.device_metrics_hourly WHERE device_id = $1 AND hour = to_timestamp($2)")
        .bind(server_device).bind(old_hour as f64).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(rolled, (2, 200, 300, 40, 60));
    let (minutes, _) = enrollment.expire(30, 13).await.unwrap();
    assert_eq!(minutes, 2);
    stored(admin, server_device, 135).await;

    // Inventory: full reconciliation, change sets, restart and a refused change set.
    let store = std::env::temp_dir().join(format!("midas-inventory-test-{}.pb", Uuid::new_v4()));
    let current = Arc::new(std::sync::Mutex::new(software(450)));
    let collector: midas_agent_core::inventory::Collector = {
        let current = current.clone();
        Arc::new(move || current.lock().unwrap().clone())
    };
    let tls = deployment
        .client_config(roots(&server_ca), &preapproved.certificate)
        .unwrap();
    let shared = midas_agent_core::inventory::shared(store.clone());
    let mut inventoried = Agent::with(
        proxy_port,
        tls.clone(),
        None,
        Some((shared.clone(), collector.clone())),
    );
    let (generation, reconciled) =
        inventory_state(admin, server_device, |items| items.len() == 451).await;
    assert!(generation.is_some() && reconciled.is_some());

    {
        let mut snapshot = current.lock().unwrap();
        snapshot.remove(&midas_agent_core::inventory::key(
            "software",
            "package-0000",
        ));
        midas_agent_core::inventory::insert(&mut snapshot, "software", "new-package", "1.0");
        midas_agent_core::inventory::insert(&mut snapshot, "software", "package-0001", "2.0");
    }
    midas_agent_core::inventory::lock(&shared).wake.notify_one();
    let changed = inventory_state(admin, server_device, |items| {
        items.get("new-package").is_some()
            && items.get("package-0000").is_none()
            && items.get("package-0001").map(String::as_str) == Some("2.0")
    })
    .await;
    assert_eq!(
        changed,
        (generation, reconciled),
        "a change set is not a reconciliation"
    );

    inventoried.stop.send_replace(true);
    inventoried
        .reach("inventory agent stop", |s| *s == Status::Stopped)
        .await;
    midas_agent_core::inventory::insert(
        &mut current.lock().unwrap(),
        "software",
        "after-restart",
        "1",
    );
    let restarted = midas_agent_core::inventory::shared(store.clone());
    let inventoried = Agent::with(
        proxy_port,
        tls.clone(),
        None,
        Some((restarted.clone(), collector.clone())),
    );
    let after = inventory_state(admin, server_device, |items| {
        items.get("after-restart").is_some()
    })
    .await;
    assert_eq!(
        after,
        (generation, reconciled),
        "a restart resumes from the last reconciliation"
    );

    sqlx::query("UPDATE midas.devices SET inventory_generation = 999999 WHERE id = $1")
        .bind(server_device)
        .execute(&mut *admin)
        .await
        .unwrap();
    sqlx::query("INSERT INTO midas.device_inventory (tenant_id, device_id, kind, key, value, generation) VALUES ($1, $2, 'software', 'stale', 'x', 999999)")
        .bind(tenant).bind(server_device).execute(&mut *admin).await.unwrap();
    midas_agent_core::inventory::insert(&mut current.lock().unwrap(), "software", "trigger", "1");
    midas_agent_core::inventory::lock(&restarted)
        .wake
        .notify_one();
    let expected = current.lock().unwrap().len();
    let (full_generation, full_reconciled) = inventory_state(admin, server_device, |items| {
        items.get("trigger").is_some() && items.get("stale").is_none() && items.len() == expected
    })
    .await;
    assert!(full_generation.is_some() && full_generation != generation);
    assert!(
        full_reconciled > reconciled,
        "the refused change set forced a full reconciliation"
    );

    let path = format!("/api/tenants/{tenant}/devices/{server_device}/inventory");
    let (status, view, _) = call(&app, "GET", &path, Some(&manager), None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["items"].as_array().unwrap().len(), expected);
    assert!(view["reconciled_at"].as_i64().is_some());
    let (status, _, _) = call(&app, "GET", &path, Some(&outsider), None, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    inventoried.stop.send_replace(true);
    let _ = std::fs::remove_file(&store);

    // Health: every result is stored, an unavailable check stays visible, and only a change
    // of state or reason moves its change time.
    use midas_protocol::HealthState::{Critical, Ok as Healthy, Unavailable, Warning};
    let results = Arc::new(std::sync::Mutex::new(vec![
        health_check(
            "services",
            Critical,
            "stopped",
            &["Print Spooler (Spooler)"],
        ),
        health_check("updates", Warning, "reboot_pending", &[]),
        health_check("antivirus", Unavailable, "not_supported", &[]),
        health_check("encryption", Healthy, "encrypted", &[]),
        health_check("storage", Warning, "low_space", &["/data (8% free)"]),
    ]));
    let checker: midas_agent_core::health::Checker = {
        let results = results.clone();
        Arc::new(move |_: &midas_agent_core::health::Thresholds| results.lock().unwrap().clone())
    };
    let health = midas_agent_core::health::shared();
    let capabilities: Vec<String> = [
        "metrics",
        "health.services",
        "health.updates",
        "health.encryption",
        "health.storage",
    ]
    .map(String::from)
    .to_vec();
    let checked = Agent::checked(
        proxy_port,
        tls.clone(),
        health.clone(),
        checker,
        capabilities.clone(),
    );
    let first = health_rows(admin, server_device, |rows| rows.len() == 5).await;
    let stored: Vec<(&str, &str, &str)> = first
        .iter()
        .map(|(id, state, code, _, _)| (id.as_str(), state.as_str(), code.as_str()))
        .collect();
    assert_eq!(
        stored,
        [
            ("antivirus", "unavailable", "not_supported"),
            ("encryption", "ok", "encrypted"),
            ("services", "critical", "stopped"),
            ("storage", "warning", "low_space"),
            ("updates", "warning", "reboot_pending"),
        ]
    );
    assert_eq!(first[2].3, ["Print Spooler (Spooler)"]);
    let path = format!("/api/tenants/{tenant}/devices/{server_device}/health");
    let (status, view, _) = call(&app, "GET", &path, Some(&manager), None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["capabilities"], json!(capabilities));
    assert!(view["reported_at"].as_i64().is_some());
    assert_eq!(view["checks"][0]["id"], "antivirus");
    assert_eq!(view["checks"][0]["state"], "unavailable");
    let (status, _, _) = call(&app, "GET", &path, Some(&outsider), None, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    tokio::time::sleep(Duration::from_millis(50)).await;
    let changed = vec![
        health_check("services", Healthy, "running", &[]),
        health_check("updates", Warning, "reboot_pending", &[]),
        health_check("antivirus", Unavailable, "not_supported", &[]),
        health_check("storage", Warning, "low_space", &["/data (7% free)"]),
    ];
    *results.lock().unwrap() = changed.clone();
    midas_agent_core::health::lock(&health).publish(changed, std::time::Instant::now());
    let second = health_rows(admin, server_device, |rows| {
        rows.len() == 4 && rows.iter().any(|row| row.0 == "services" && row.1 == "ok")
    })
    .await;
    let time = |rows: &[HealthRow], id: &str| rows.iter().find(|row| row.0 == id).unwrap().4;
    assert!(
        time(&second, "services") > time(&first, "services"),
        "a new state moves the change time"
    );
    assert_eq!(
        time(&second, "storage"),
        time(&first, "storage"),
        "a new subject alone does not"
    );
    assert_eq!(
        second.iter().find(|row| row.0 == "storage").unwrap().3,
        ["/data (7% free)"]
    );
    assert!(
        second.iter().all(|row| row.0 != "encryption"),
        "a check no longer reported is removed"
    );
    checked.stop.send_replace(true);

    // Policies: a group override reaches only its members, shows its source and reaches the
    // live agent, which re-runs its checks with the new limit.
    let low = midas_protocol::policy::LOW_SPACE_PERCENT.key;
    let api =
        |method: &'static str, path: String, client: &Client, body: Option<serde_json::Value>| {
            let (app, client) = (app.clone(), client.clone());
            async move { call(&app, method, &path, Some(&client), body, &[]).await }
        };
    let effective = |target: Uuid| {
        let manager = manager.clone();
        async move {
            let (status, settings, _) = api(
                "GET",
                format!("/api/tenants/{tenant}/devices/{target}/policy"),
                &manager,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let setting = settings
                .as_array()
                .unwrap()
                .iter()
                .find(|setting| setting["key"] == low)
                .unwrap()
                .clone();
            (
                setting["value"].as_i64().unwrap(),
                setting["source"]["level"].as_str().unwrap().to_owned(),
                setting["source"]["name"].as_str().map(str::to_owned),
            )
        }
    };
    assert_eq!(effective(server_device).await, (10, "default".into(), None));
    let tenant_value = format!("/api/tenants/{tenant}/policies/tenant/{tenant}/{low}");
    let (status, _, _) = api(
        "PUT",
        tenant_value.clone(),
        &outsider,
        Some(json!({"value": 15})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    for (path, value) in [
        (tenant_value.clone(), 0),
        (
            format!("/api/tenants/{tenant}/policies/tenant/{tenant}/health.unknown"),
            5,
        ),
        (
            format!("/api/tenants/{tenant}/policies/tenant/{other}/{low}"),
            15,
        ),
    ] {
        let (status, _, _) = api("PUT", path, &manager, Some(json!({"value": value}))).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
            "{status}"
        );
    }
    let (status, _, _) = api(
        "PUT",
        tenant_value.clone(),
        &manager,
        Some(json!({"value": 15})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let group = |name: &str, priority: i32| {
        let manager = manager.clone();
        let body = json!({"name": name, "priority": priority});
        async move {
            let (status, created, _) = api(
                "POST",
                format!("/api/tenants/{tenant}/groups"),
                &manager,
                Some(body),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
            created["id"].as_str().unwrap().parse::<Uuid>().unwrap()
        }
    };
    let servers = group("Servers", 10).await;
    let everyone = group("Everyone", 0).await;
    for (group, member) in [
        (servers, server_device),
        (everyone, server_device),
        (everyone, device),
    ] {
        let path = format!("/api/tenants/{tenant}/groups/{group}/devices/{member}");
        let (status, _, _) = api("PUT", path, &manager, None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    let unknown = format!(
        "/api/tenants/{tenant}/groups/{servers}/devices/{}",
        Uuid::new_v4()
    );
    assert_eq!(
        api("PUT", unknown, &manager, None).await.0,
        StatusCode::NOT_FOUND
    );
    let servers_value = format!("/api/tenants/{tenant}/policies/group/{servers}/{low}");
    let (status, _, _) = api(
        "PUT",
        servers_value.clone(),
        &manager,
        Some(json!({"value": 25})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        effective(server_device).await,
        (25, "group".into(), Some("Servers".into()))
    );
    assert_eq!(
        effective(device).await,
        (15, "tenant".into(), Some("Enrollment A".into())),
        "the group override does not reach a device outside the group"
    );
    let everyone_value = format!("/api/tenants/{tenant}/policies/group/{everyone}/{low}");
    let (status, _, _) = api("PUT", everyone_value, &manager, Some(json!({"value": 30}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        effective(server_device).await,
        (25, "group".into(), Some("Servers".into())),
        "the higher-priority group wins"
    );
    assert_eq!(
        effective(device).await,
        (30, "group".into(), Some("Everyone".into()))
    );

    let limits: midas_agent_core::health::Checker =
        Arc::new(|limits: &midas_agent_core::health::Thresholds| {
            vec![health_check(
                "storage",
                midas_protocol::HealthState::Ok,
                "healthy",
                &[&limits.low_space_percent.to_string()],
            )]
        });
    let policed = Agent::checked(
        proxy_port,
        tls.clone(),
        midas_agent_core::health::shared(),
        limits,
        Vec::new(),
    );
    let reported = |expected: &'static str| {
        move |rows: &[HealthRow]| rows.len() == 1 && rows[0].3 == [expected]
    };
    health_rows(admin, server_device, reported("25")).await;
    let (status, _, _) = api("DELETE", servers_value.clone(), &manager, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        api("DELETE", servers_value, &manager, None).await.0,
        StatusCode::NOT_FOUND
    );
    health_rows(admin, server_device, reported("30")).await;
    let membership = format!("/api/tenants/{tenant}/groups/{everyone}/devices/{server_device}");
    assert_eq!(
        api("DELETE", membership.clone(), &manager, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api("DELETE", membership, &manager, None).await.0,
        StatusCode::NOT_FOUND
    );
    health_rows(admin, server_device, reported("15")).await;
    assert_eq!(
        effective(server_device).await,
        (15, "tenant".into(), Some("Enrollment A".into()))
    );
    let path = format!("/api/tenants/{tenant}/devices/{server_device}/policy");
    let (status, _, _) = call(&app, "GET", &path, Some(&outsider), None, &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, view, _) = api(
        "GET",
        format!("/api/tenants/{tenant}/policies"),
        &manager,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["values"].as_array().unwrap().len(), 2);
    policed.stop.send_replace(true);
    let policy_events: Vec<(String, i64)> = sqlx::query_as("SELECT action, count(*) FROM midas.tenant_audit WHERE tenant_id = $1 AND (action LIKE 'policy.%' OR action LIKE 'inventory.group%') GROUP BY action ORDER BY action")
        .bind(tenant).fetch_all(&mut *admin).await.unwrap();
    assert_eq!(
        policy_events,
        [
            ("inventory.group_create".to_owned(), 2),
            ("inventory.group_member_add".to_owned(), 3),
            ("inventory.group_member_remove".to_owned(), 1),
            ("policy.clear".to_owned(), 1),
            ("policy.set".to_owned(), 3),
        ]
    );

    // Alerts: health problems opened and resolved alerts on their own; an outage opens one
    // alert and one notification; repeats, a quick flap and the policy delay add nothing more.
    let health_alerts: Vec<(String, String, bool)> = sqlx::query_as("SELECT kind, severity, resolved_at IS NOT NULL FROM midas.alerts WHERE device_id = $1 ORDER BY kind")
        .bind(server_device).fetch_all(&mut *admin).await.unwrap();
    let resolved = |kind: &str, severity: &str| (kind.to_owned(), severity.to_owned(), true);
    assert_eq!(
        health_alerts,
        [
            resolved("health.services", "critical"),
            resolved("health.storage", "warning"),
            resolved("health.updates", "warning"),
        ]
    );
    let recipients = format!("/api/tenants/{tenant}/alert-recipients");
    let emails = |emails: &[&str]| Some(json!({ "emails": emails }));
    assert_eq!(
        api(
            "PUT",
            recipients.clone(),
            &outsider,
            emails(&["ops@example.test"])
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(
            "PUT",
            recipients.clone(),
            &manager,
            emails(&["not an address"])
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let both = emails(&["ops@example.test", "ops@example.test"]);
    assert_eq!(
        api("PUT", recipients.clone(), &manager, both).await.0,
        StatusCode::NO_CONTENT
    );
    let (_, stored, _) = api("GET", recipients, &manager, None).await;
    assert_eq!(stored["emails"], json!(["ops@example.test"]));

    let offline = |minutes: i32| {
        sqlx::query("UPDATE midas.devices SET online = false, last_seen_at = clock_timestamp() - make_interval(mins => $2) WHERE id = $1")
            .bind(server_device)
            .bind(minutes)
    };
    let open_offline = "SELECT count(*) FROM midas.alerts WHERE device_id = $1 AND kind = 'device.offline' AND resolved_at IS NULL";
    let delay = format!(
        "/api/tenants/{tenant}/policies/tenant/{tenant}/{}",
        midas_protocol::policy::OFFLINE_ALERT_MINUTES.key
    );
    assert_eq!(
        api("PUT", delay.clone(), &manager, Some(json!({"value": 30})))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    offline(10).execute(&mut *admin).await.unwrap();
    assert_eq!(
        enrollment.alert_sweep().await.unwrap().0,
        0,
        "the policy delay is 30 minutes"
    );
    assert_eq!(
        api("DELETE", delay, &manager, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        enrollment.alert_sweep().await.unwrap().0,
        1,
        "the default delay is 5 minutes"
    );
    assert_eq!(enrollment.alert_sweep().await.unwrap().0, 0);
    let open: i64 = sqlx::query_scalar(open_offline)
        .bind(server_device)
        .fetch_one(&mut *admin)
        .await
        .unwrap();
    assert_eq!(open, 1);

    let (smtp_port, inbox) = mail_sink().await;
    let mailer = midas_server::alerts::Mailer::smtp(&midas_server::alerts::SmtpSettings {
        host: "127.0.0.1".into(),
        port: smtp_port,
        security: midas_server::alerts::SmtpSecurity::None,
        credentials: None,
        from: "MIDAS <midas@example.test>".into(),
    })
    .unwrap();
    assert_eq!(enrollment.deliver(&mailer).await.unwrap(), 1);
    assert_eq!(enrollment.deliver(&mailer).await.unwrap(), 0);
    {
        let inbox = inbox.lock().unwrap();
        assert_eq!(inbox.len(), 1);
        assert!(
            inbox[0].contains("Subject: [MIDAS] Critical: ") && inbox[0].contains("Device offline")
        );
    }

    sqlx::query("UPDATE midas.devices SET online = true WHERE id = $1")
        .bind(server_device)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert_eq!(
        enrollment.alert_sweep().await.unwrap().1,
        1,
        "back online resolves it"
    );
    offline(10).execute(&mut *admin).await.unwrap();
    assert_eq!(
        enrollment.alert_sweep().await.unwrap().0,
        0,
        "a quick flap reopens the same alert"
    );
    assert_eq!(enrollment.deliver(&mailer).await.unwrap(), 0);
    assert_eq!(
        inbox.lock().unwrap().len(),
        1,
        "one outage, one notification"
    );

    let (status, listed, _) = api(
        "GET",
        format!("/api/tenants/{tenant}/alerts/open"),
        &manager,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (
            &listed[0]["kind"],
            &listed[0]["occurrences"],
            &listed[0]["notifications_sent"]
        ),
        (&json!("device.offline"), &json!(2), &json!(1))
    );
    let (status, _, _) = api(
        "GET",
        format!("/api/tenants/{tenant}/alerts"),
        &outsider,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let acknowledge = format!(
        "/api/tenants/{tenant}/alerts/{}/acknowledge",
        listed[0]["id"].as_str().unwrap()
    );
    assert_eq!(
        api("POST", acknowledge.clone(), &outsider, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api("POST", acknowledge.clone(), &manager, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api("POST", acknowledge, &manager, None).await.0,
        StatusCode::CONFLICT
    );
    let alert_events: Vec<(String, i64)> = sqlx::query_as("SELECT action, count(*) FROM midas.tenant_audit WHERE tenant_id = $1 AND action LIKE 'alert.%' GROUP BY action ORDER BY action")
        .bind(tenant).fetch_all(&mut *admin).await.unwrap();
    assert_eq!(
        alert_events,
        [
            ("alert.acknowledge".to_owned(), 1),
            ("alert.recipients_set".to_owned(), 1)
        ]
    );
    sqlx::query("UPDATE midas.devices SET online = true WHERE id = $1")
        .bind(server_device)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert_eq!(enrollment.alert_sweep().await.unwrap().1, 1);
    let connector_sets: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenant_audit WHERE tenant_id = $1 AND action = 'directory.connector_set' AND outcome = 'success'")
        .bind(tenant).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(connector_sets, 2);

    let (_, created, _) = create(&manager, tenant, spec.clone()).await;
    sqlx::query("UPDATE midas.enrollment_tokens SET created_at = clock_timestamp() - interval '2 hours', expires_at = clock_timestamp() - interval '1 hour' WHERE id = $1")
        .bind(created["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .execute(&mut *admin)
        .await
        .unwrap();
    assert_eq!(
        enroll(
            &server,
            &request(
                created["token"].as_str().unwrap(),
                Platform::Windows,
                Kind::Workstation
            )
        )
        .await,
        Err(EnrollError::Rejected),
        "an expired token must be refused"
    );

    let actions: Vec<(String, String, i64)> = sqlx::query_as("SELECT action, outcome, count(*) FROM midas.tenant_audit WHERE tenant_id = $1 AND action LIKE 'device.%' GROUP BY action, outcome ORDER BY action, outcome")
        .bind(tenant).fetch_all(&mut *admin).await.unwrap();
    let count = |action: &str, outcome: &str| {
        actions
            .iter()
            .find(|(a, o, _)| a == action && o == outcome)
            .map_or(0, |(_, _, n)| *n)
    };
    assert_eq!(count("device.enroll_token_create", "success"), 4);
    assert_eq!(count("device.enroll_request", "success"), 3);
    assert_eq!(count("device.enroll_request", "denied"), 4);
    assert_eq!(count("device.enroll_approve", "success"), 1);
    assert_eq!(count("device.enroll_reject", "success"), 1);
    assert_eq!(count("device.revoke", "success"), 1);
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM midas.tenant_audit WHERE details::text LIKE '%midas-enroll-%'",
    )
    .fetch_one(&mut *admin)
    .await
    .unwrap();
    assert_eq!(leaked, 0, "tokens must not reach the audit trail");
}
