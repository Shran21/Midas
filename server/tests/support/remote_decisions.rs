use std::time::Duration;

use axum::http::StatusCode;
use futures_util::{SinkExt, StreamExt};
use midas_protocol::{
    Body,
    sessions::{OsSession, Presence, SessionReport},
};
use midas_server::database::driver as sqlx;
use serde_json::json;
use sqlx::PgConnection;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use uuid::Uuid;

use super::{administration::call, remote_sessions::Context};

pub async fn verify(
    ctx: &Context<'_>,
    admin: &mut PgConnection,
    socket: &mut WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
    connection: Uuid,
    operator: Uuid,
) {
    let path = format!(
        "/api/tenants/{}/devices/{}/unattended-check",
        ctx.tenant, ctx.device
    );
    let tenant_path = format!("/api/tenants/{}/unattended-policy", ctx.tenant);
    let device_path = format!(
        "/api/tenants/{}/devices/{}/unattended-policy",
        ctx.tenant, ctx.device
    );
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let original_tenant = call(ctx.app, "GET", &tenant_path, Some(ctx.manager), None, &[])
        .await
        .1;
    let original_device = call(ctx.app, "GET", &device_path, Some(ctx.manager), None, &[])
        .await
        .1;
    for (path, body) in [
        (&tenant_path, json!({"enabled":true})),
        (
            &device_path,
            json!({"enabled":true,"maintenance_window":{"starts_at":now-60,"ends_at":now+600}}),
        ),
    ] {
        let (status, body, _) =
            call(ctx.app, "PUT", path, Some(ctx.manager), Some(body), &[]).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    let generation = Uuid::new_v4();
    let body = json!({"mode":"administrative","uac":true,"reason":"Scheduled maintenance",
        "session":{"connection_id":connection,"os_session_id":"7","generation":generation}});
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &path,
            Some(ctx.manager),
            Some(body.clone()),
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE midas.devices SET platform='windows' WHERE tenant_id=$1 AND id=$2")
        .bind(ctx.tenant)
        .bind(ctx.device)
        .execute(&mut *admin)
        .await
        .unwrap();
    for (client, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(ctx.outsider), StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            call(ctx.app, "POST", &path, client, Some(body.clone()), &[])
                .await
                .0,
            expected
        );
    }
    use tower::ServiceExt;
    let no_csrf = axum::http::Request::builder()
        .method("POST")
        .uri(&path)
        .extension(axum::extract::ConnectInfo(
            "198.51.100.93:40000"
                .parse::<std::net::SocketAddr>()
                .unwrap(),
        ))
        .header("origin", super::administration::ORIGIN)
        .header("cookie", ctx.manager.cookie())
        .header("content-type", "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();
    assert_eq!(
        ctx.app.clone().oneshot(no_csrf).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    for nested in [false, true] {
        let mut forged = body.clone();
        if nested {
            forged["session"]["presence"] = json!("vacant");
        } else {
            forged["presence"] = json!("vacant");
        }
        assert_eq!(
            call(ctx.app, "POST", &path, Some(ctx.manager), Some(forged), &[])
                .await
                .0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let mut invalid = body.clone();
    invalid["reason"] = json!(" ");
    assert_eq!(
        call(
            ctx.app,
            "POST",
            &path,
            Some(ctx.manager),
            Some(invalid),
            &[]
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let mut stale = body.clone();
    stale["session"]["connection_id"] = json!(Uuid::new_v4());
    assert_eq!(
        call(ctx.app, "POST", &path, Some(ctx.manager), Some(stale), &[])
            .await
            .0,
        StatusCode::CONFLICT
    );
    sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp()-interval '6 minutes' WHERE operator_id=$1").bind(operator).execute(&mut *admin).await.unwrap();
    let denied = call(
        ctx.app,
        "POST",
        &path,
        Some(ctx.manager),
        Some(body.clone()),
        &[],
    )
    .await;
    assert_eq!(denied.0, StatusCode::FORBIDDEN);
    assert_eq!(denied.1["error"], "fresh_passkey_required");
    sqlx::query(
        "UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp() WHERE operator_id=$1",
    )
    .bind(operator)
    .execute(&mut *admin)
    .await
    .unwrap();

    for scenario in [
        "vacant",
        "occupied",
        "unknown",
        "replaced",
        "gone",
        "disabled",
        "denied",
        "credential",
        "expired",
        "audit",
        "disconnected",
    ] {
        let (app, client, path, sent) = (
            ctx.app.clone(),
            ctx.manager.clone(),
            path.clone(),
            body.clone(),
        );
        let pending =
            tokio::spawn(
                async move { call(&app, "POST", &path, Some(&client), Some(sent), &[]).await },
            );
        let request = tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                tokio::select! {
                    next = socket.next() => {
                        if let Message::Binary(bytes) = next.unwrap().unwrap()
                            && let Some(Body::SessionRequest(request)) = midas_protocol::decode(&bytes).unwrap() {
                            break request;
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {
                        socket.send(Message::Binary(midas_protocol::encode(Body::Ping(midas_protocol::Ping { nonce: 9 })).into())).await.unwrap();
                    }
                }
            }
        }).await.unwrap_or_else(|_| panic!("{scenario}: no native query"));
        assert_eq!(request.connection_id, connection.as_bytes());
        assert!(!pending.is_finished());
        let mut report = SessionReport {
            request_id: request.request_id,
            connection_id: request.connection_id,
            available: true,
            sessions: vec![OsSession {
                id: 7,
                generation: generation.as_bytes().to_vec(),
                presence: Presence::Vacant as i32,
                station: "Console".into(),
                console: true,
                ..Default::default()
            }],
        };
        match scenario {
            "occupied" => {
                report.sessions[0].presence = Presence::Occupied as i32;
                report.sessions[0].account = "EXAMPLE\\operator".into();
            }
            "unknown" => report.sessions[0].presence = Presence::Unknown as i32,
            "replaced" => report.sessions[0].generation = Uuid::new_v4().as_bytes().to_vec(),
            "gone" => report.sessions.clear(),
            "disabled" => {
                sqlx::query("UPDATE midas.tenants SET unattended_enabled=false WHERE id=$1")
                    .bind(ctx.tenant)
                    .execute(&mut *admin)
                    .await
                    .unwrap();
            }
            "denied" => {
                sqlx::query("UPDATE midas.tenant_memberships SET enabled=false WHERE tenant_id=$1 AND operator_id=$2").bind(ctx.tenant).bind(operator).execute(&mut *admin).await.unwrap();
            }
            "credential" => {
                sqlx::query("UPDATE midas.agent_credentials SET revoked_at=clock_timestamp() WHERE tenant_id=$1 AND device_id=$2").bind(ctx.tenant).bind(ctx.device).execute(&mut *admin).await.unwrap();
            }
            "expired" => {
                sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp()-interval '6 minutes' WHERE operator_id=$1").bind(operator).execute(&mut *admin).await.unwrap();
            }
            "audit" => {
                sqlx::raw_sql("CREATE FUNCTION midas.reject_decision_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='remote.request' THEN RAISE EXCEPTION 'test'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_decision_audit BEFORE INSERT ON midas.tenant_audit FOR EACH ROW EXECUTE FUNCTION midas.reject_decision_audit();").execute(&mut *admin).await.unwrap();
            }
            _ => {}
        }
        if scenario == "disconnected" {
            socket.close(None).await.unwrap();
        } else {
            socket
                .send(Message::Binary(
                    midas_protocol::encode(Body::SessionReport(report)).into(),
                ))
                .await
                .unwrap();
        }
        let (status, response, _) = tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap();
        let expected = match scenario {
            "vacant" | "occupied" => StatusCode::OK,
            "denied" | "disabled" => StatusCode::NOT_FOUND,
            "expired" => StatusCode::FORBIDDEN,
            "audit" => StatusCode::SERVICE_UNAVAILABLE,
            _ => StatusCode::CONFLICT,
        };
        assert_eq!(status, expected, "{scenario}: {response}");
        if scenario == "vacant" {
            assert_eq!(response["decision"], "eligible");
            assert_eq!(response["session"], body["session"]);
            let expiry = response["expires_at"].as_i64().unwrap();
            let current = time::OffsetDateTime::now_utc().unix_timestamp();
            assert!(expiry > current && expiry <= current + 30);
            assert_eq!(response.as_object().unwrap().len(), 3);
        }
        if scenario == "occupied" {
            assert_eq!(response, json!({"decision":"local_consent_required"}));
        }
        match scenario {
            "disabled" => {
                sqlx::query("UPDATE midas.tenants SET unattended_enabled=true WHERE id=$1")
                    .bind(ctx.tenant)
                    .execute(&mut *admin)
                    .await
                    .unwrap();
            }
            "denied" => {
                sqlx::query("UPDATE midas.tenant_memberships SET enabled=true WHERE tenant_id=$1 AND operator_id=$2").bind(ctx.tenant).bind(operator).execute(&mut *admin).await.unwrap();
            }
            "credential" => {
                sqlx::query("UPDATE midas.agent_credentials SET revoked_at=NULL WHERE tenant_id=$1 AND device_id=$2").bind(ctx.tenant).bind(ctx.device).execute(&mut *admin).await.unwrap();
            }
            "expired" => {
                sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp() WHERE operator_id=$1").bind(operator).execute(&mut *admin).await.unwrap();
            }
            "audit" => {
                sqlx::raw_sql("DROP TRIGGER reject_decision_audit ON midas.tenant_audit; DROP FUNCTION midas.reject_decision_audit();").execute(&mut *admin).await.unwrap();
            }
            _ => {}
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenant_audit WHERE tenant_id=$1 AND device_id=$2 AND action='remote.request' AND outcome='success' AND details->>'reason'='Scheduled maintenance'")
        .bind(ctx.tenant).bind(ctx.device).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(count, 1);
    for (path, body) in [
        (
            &device_path,
            json!({"enabled": original_device["enabled"], "maintenance_window": original_device["maintenance_window"]}),
        ),
        (&tenant_path, original_tenant),
    ] {
        assert_eq!(
            call(ctx.app, "PUT", path, Some(ctx.manager), Some(body), &[])
                .await
                .0,
            StatusCode::NO_CONTENT
        );
    }
    sqlx::query("UPDATE midas.devices SET platform='linux' WHERE tenant_id=$1 AND id=$2")
        .bind(ctx.tenant)
        .bind(ctx.device)
        .execute(&mut *admin)
        .await
        .unwrap();
}
