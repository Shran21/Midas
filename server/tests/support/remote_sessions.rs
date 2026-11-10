use std::{sync::Arc, time::Duration};

use axum::{Router, http::StatusCode};
use futures_util::{SinkExt, StreamExt};
use midas_protocol::{
    Body, Hello,
    sessions::{CAPABILITY, OsSession, Presence, SessionReport},
};
use midas_server::{auth::http::WebAuth, database::driver as sqlx};
use rustls::ClientConfig;
use sqlx::PgConnection;
use tokio_tungstenite::{client_async, tungstenite::Message};
use uuid::Uuid;

use super::administration::{Client, ORIGIN, call};

pub struct Context<'a> {
    pub app: &'a Router,
    pub web: &'a Arc<WebAuth>,
    pub manager: &'a Client,
    pub outsider: &'a Client,
    pub tenant: Uuid,
    pub device: Uuid,
    pub port: u16,
    pub tls: Arc<ClientConfig>,
}

pub async fn verify(ctx: Context<'_>, admin: &mut PgConnection) {
    let path = format!(
        "/api/tenants/{}/devices/{}/remote-sessions",
        ctx.tenant, ctx.device
    );
    assert_eq!(
        call(ctx.app, "POST", &path, None, None, &[]).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(ctx.app, "POST", &path, Some(ctx.outsider), None, &[])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let token = midas_server::auth::sessions::SessionToken::parse(
        ctx.manager.cookie().split_once('=').unwrap().1,
    )
    .unwrap();
    let operator = ctx
        .web
        .sessions()
        .validate(&token, false)
        .await
        .unwrap()
        .operator_id;
    let tcp = tokio::net::TcpStream::connect(("127.0.0.1", ctx.port))
        .await
        .unwrap();
    let tls = tokio_rustls::TlsConnector::from(ctx.tls.clone())
        .connect(
            rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap(),
            tcp,
        )
        .await
        .unwrap();
    let (mut socket, _) = client_async(format!("wss://127.0.0.1:{}/agent/connect", ctx.port), tls)
        .await
        .unwrap();
    socket
        .send(Message::Binary(
            midas_protocol::encode(Body::Hello(Hello {
                min_version: 1,
                max_version: 1,
                agent_version: "test".into(),
                platform: "windows".into(),
                capabilities: vec![CAPABILITY.into()],
            }))
            .into(),
        ))
        .await
        .unwrap();
    let welcome = loop {
        let Message::Binary(bytes) = socket.next().await.unwrap().unwrap() else {
            continue;
        };
        if let Some(Body::Welcome(welcome)) = midas_protocol::decode(&bytes).unwrap() {
            break welcome;
        }
    };
    let connection = Uuid::from_slice(&welcome.connection_id).unwrap();
    assert!(!connection.is_nil());
    let issue = || {
        let (app, client, path) = (ctx.app.clone(), ctx.manager.clone(), path.clone());
        tokio::spawn(async move { call(&app, "POST", &path, Some(&client), None, &[]).await })
    };
    let mut previous = None;
    for scenario in [
        "normal",
        "replay",
        "denied",
        "credential",
        "audit",
        "unavailable",
        "invalid",
        "timeout",
    ] {
        let mut pending = issue();
        let request = tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                tokio::select! {
                    result = &mut pending => {
                        let (status, body, _) = result.unwrap();
                        assert!(scenario == "normal" && status == StatusCode::CONFLICT, "{scenario}: premature {status}: {body}");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        pending = issue();
                    }
                    next = socket.next() => {
                        let next = next.expect("agent stream").unwrap_or_else(|error| panic!("{scenario}: {error}"));
                        if let Message::Binary(bytes) = next
                            && let Some(Body::SessionRequest(request)) = midas_protocol::decode(&bytes).unwrap()
                        {
                            break request;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {
                        socket.send(Message::Binary(midas_protocol::encode(Body::Ping(midas_protocol::Ping { nonce: 1 })).into())).await.unwrap();
                    }
                }
            }
        }).await.unwrap();
        assert_eq!(request.connection_id, welcome.connection_id);
        let report = SessionReport {
            request_id: request.request_id.clone(),
            connection_id: request.connection_id.clone(),
            available: true,
            sessions: vec![OsSession {
                id: 7,
                generation: Uuid::new_v4().as_bytes().to_vec(),
                presence: Presence::Occupied as i32,
                account: "EXAMPLE\\operator".into(),
                station: "RDP-Tcp#3".into(),
                console: false,
                locked: Some(true),
            }],
        };
        if scenario == "normal" {
            let concurrent = issue().await.unwrap();
            assert_eq!(concurrent.0, StatusCode::CONFLICT);
            let request = axum::http::Request::builder()
                .method("POST")
                .uri(&path)
                .extension(axum::extract::ConnectInfo(
                    "198.51.100.93:40000"
                        .parse::<std::net::SocketAddr>()
                        .unwrap(),
                ))
                .header("origin", ORIGIN)
                .header("cookie", ctx.manager.cookie())
                .body(axum::body::Body::empty())
                .unwrap();
            use tower::ServiceExt;
            assert_eq!(
                ctx.app.clone().oneshot(request).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
        if scenario == "replay" {
            socket
                .send(Message::Binary(
                    midas_protocol::encode(Body::SessionReport(previous.take().unwrap())).into(),
                ))
                .await
                .unwrap();
            let mut wrong = report.clone();
            wrong.connection_id = Uuid::new_v4().as_bytes().to_vec();
            socket
                .send(Message::Binary(
                    midas_protocol::encode(Body::SessionReport(wrong)).into(),
                ))
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(!pending.is_finished());
        }
        if scenario == "denied" {
            sqlx::query("UPDATE midas.tenant_memberships SET enabled=false WHERE tenant_id=$1 AND operator_id=$2").bind(ctx.tenant).bind(operator).execute(&mut *admin).await.unwrap();
        }
        if scenario == "credential" {
            sqlx::query("UPDATE midas.agent_credentials SET revoked_at=now() WHERE tenant_id=$1 AND device_id=$2").bind(ctx.tenant).bind(ctx.device).execute(&mut *admin).await.unwrap();
        }
        if scenario == "audit" {
            sqlx::raw_sql("CREATE FUNCTION midas.reject_session_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='remote.sessions_read' THEN RAISE EXCEPTION 'test'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_session_audit BEFORE INSERT ON midas.tenant_audit FOR EACH ROW EXECUTE FUNCTION midas.reject_session_audit();").execute(&mut *admin).await.unwrap();
        }
        let mut sent = report.clone();
        if scenario == "unavailable" {
            sent.available = false;
            sent.sessions.clear();
        }
        if scenario == "invalid" {
            sent.sessions[0].generation = vec![0; 16];
        }
        if scenario != "timeout" {
            socket
                .send(Message::Binary(
                    midas_protocol::encode(Body::SessionReport(sent)).into(),
                ))
                .await
                .unwrap();
        }
        let started = tokio::time::Instant::now();
        let complete = async {
            loop {
                tokio::select! {
                    result = &mut pending => break result,
                    _ = tokio::time::sleep(Duration::from_millis(500)), if scenario == "timeout" => {
                        socket.send(Message::Binary(midas_protocol::encode(Body::Ping(midas_protocol::Ping { nonce: 1 })).into())).await.unwrap();
                    }
                }
            }
        };
        let (status, body, _) = tokio::time::timeout(Duration::from_secs(12), complete)
            .await
            .unwrap()
            .unwrap();
        let expected = match scenario {
            "normal" | "replay" => StatusCode::OK,
            "denied" => StatusCode::NOT_FOUND,
            "audit" => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::CONFLICT,
        };
        assert_eq!(status, expected, "{scenario}: {body}");
        if scenario == "timeout" {
            assert!(started.elapsed() >= Duration::from_secs(7));
        }
        if status == StatusCode::OK {
            assert_eq!(body["connection_id"], connection.to_string());
            assert_eq!(
                body["sessions"][0]["selection"]["connection_id"],
                connection.to_string()
            );
            assert_eq!(body["sessions"][0]["presence"], "occupied");
            assert_eq!(body["sessions"][0]["locked"], true);
            assert_eq!(body["sessions"][0]["selection"]["os_session_id"], "7");
        }
        if scenario == "denied" {
            sqlx::query("UPDATE midas.tenant_memberships SET enabled=true WHERE tenant_id=$1 AND operator_id=$2").bind(ctx.tenant).bind(operator).execute(&mut *admin).await.unwrap();
        }
        if scenario == "credential" {
            sqlx::query("UPDATE midas.agent_credentials SET revoked_at=NULL WHERE tenant_id=$1 AND device_id=$2").bind(ctx.tenant).bind(ctx.device).execute(&mut *admin).await.unwrap();
        }
        if scenario == "audit" {
            sqlx::raw_sql("DROP TRIGGER reject_session_audit ON midas.tenant_audit; DROP FUNCTION midas.reject_session_audit();").execute(&mut *admin).await.unwrap();
        }
        previous = Some(report);
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenant_audit WHERE tenant_id=$1 AND device_id=$2 AND action='remote.sessions_read' AND details->>'count'='1' AND NOT details::text LIKE '%EXAMPLE%'").bind(ctx.tenant).bind(ctx.device).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(count, 2);
    super::remote_decisions::verify(&ctx, admin, &mut socket, connection, operator).await;
    let _ = socket.close(None).await;

    let generation = Uuid::new_v4();
    let mut settings = midas_agent_core::connection::Settings::new(
        "127.0.0.1".into(),
        ctx.port,
        ctx.tls,
        "windows",
    );
    settings.capabilities.push(CAPABILITY.into());
    settings.sessions = Some(midas_agent_core::sessions::Probe::new(Arc::new(
        move || {
            Some(vec![OsSession {
                id: 2,
                generation: generation.as_bytes().to_vec(),
                presence: Presence::Vacant as i32,
                station: "Console".into(),
                console: true,
                ..Default::default()
            }])
        },
    )));
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let (status, _) = tokio::sync::watch::channel(midas_agent_core::connection::Status::Stopped);
    let agent = tokio::spawn(midas_agent_core::connection::run(settings, status, stopped));
    let mut accepted = None;
    for _ in 0..50 {
        let result = call(ctx.app, "POST", &path, Some(ctx.manager), None, &[]).await;
        if result.0 == StatusCode::OK {
            accepted = Some(result.1);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let view = accepted.expect("native probe response");
    assert_ne!(view["connection_id"], connection.to_string());
    assert_eq!(
        view["sessions"][0]["selection"]["generation"],
        generation.to_string()
    );
    assert_eq!(view["sessions"][0]["presence"], "vacant");
    stop.send_replace(true);
    agent.await.unwrap();
}
