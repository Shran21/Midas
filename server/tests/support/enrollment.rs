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
        let settings = Settings {
            handshake_timeout: Duration::from_secs(3),
            backoff: BackoffPolicy {
                base: Duration::from_millis(50),
                cap: Duration::from_millis(300),
            },
            ..Settings::new("127.0.0.1".into(), port, tls, "windows")
        };
        let (status_tx, status) = watch::channel(Status::Connecting { attempt: 0 });
        let (stop, stop_rx) = watch::channel(false);
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
    server_agent.stop.send_replace(true);

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
