use midas_server::database::driver as sqlx;
use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use midas_server::{
    audit::{Action, Audit, Filter},
    auth::{
        AuthConfig, AuthError,
        http::{WebAuth, router},
        sessions::SessionConfig,
        setup,
    },
    database::Database,
};
use serde_json::{Value, json};
use sqlx::PgConnection;
use tower::ServiceExt;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::*;
use zeroize::Zeroizing;

const ORIGIN: &str = "https://midas.example";

async fn call(
    app: &Router,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> (StatusCode, Value, Vec<String>) {
    let mut request = Request::builder()
        .method(if body.is_some() { "POST" } else { "GET" })
        .uri(path)
        .extension(ConnectInfo(
            "198.51.100.94:40000".parse::<SocketAddr>().unwrap(),
        ));
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cookies = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .filter(|v| !v.ends_with('='))
        .collect();
    let bytes = to_bytes(response.into_body(), 200_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookies,
    )
}

fn named<'a>(cookies: &'a [String], name: &str) -> &'a str {
    cookies.iter().find(|c| c.starts_with(name)).unwrap()
}

pub async fn verify(database: &Database, admin: &mut PgConnection) {
    let mut key = [0; 32];
    getrandom::fill(&mut key).unwrap();
    let mut config = AuthConfig::new(Url::parse(ORIGIN).unwrap(), Zeroizing::new(key));
    config.account_attempts = 300;
    config.source_attempts = 1000;
    let web = Arc::new(
        WebAuth::new(database, config, SessionConfig::default(), 4)
            .await
            .unwrap(),
    );
    let app = router(web.clone());
    let origin = [("origin", ORIGIN)];

    assert!(matches!(
        setup::issue_token(database, Duration::ZERO).await,
        Err(AuthError::InvalidInput)
    ));
    let expired = setup::issue_token(database, Duration::from_secs(600))
        .await
        .unwrap();
    sqlx::query("UPDATE midas.setup_tokens SET expires_at = clock_timestamp() - interval '1 second' WHERE used_at IS NULL")
        .execute(&mut *admin).await.unwrap();
    let start = |token: &str| json!({ "token": token, "login": "first-admin", "display_name": "Első admin" });
    assert_eq!(
        call(&app, "/api/setup/start", &origin, Some(start(&expired)))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let superseded = setup::issue_token(database, Duration::from_secs(600))
        .await
        .unwrap();
    let token = setup::issue_token(database, Duration::from_secs(600))
        .await
        .unwrap();
    assert_eq!(
        call(&app, "/api/setup/start", &origin, Some(start(&superseded)))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "/api/setup/start", &origin, Some(start("short")))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "/api/setup/start",
            &[("origin", "https://evil.example")],
            Some(start(&token))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let stored: Vec<String> =
        sqlx::query_scalar("SELECT encode(digest, 'hex') FROM midas.setup_tokens")
            .fetch_all(&mut *admin)
            .await
            .unwrap();
    assert!(stored.iter().all(|d| d != token.as_str()));

    let (status, first, cookies) =
        call(&app, "/api/setup/start", &origin, Some(start(&token))).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let abandoned = named(&cookies, "__Host-midas-login").to_owned();
    let (status, resumed, cookies) =
        call(&app, "/api/setup/start", &origin, Some(start(&token))).await;
    assert_eq!(status, StatusCode::OK);
    let binding = named(&cookies, "__Host-midas-login").to_owned();
    assert_ne!(abandoned, binding);
    let mut passkey = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let options: CreationChallengeResponse =
        serde_json::from_value(resumed["options"].clone()).unwrap();
    let response = passkey
        .do_registration(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();
    let finish = |token: &str| json!({ "token": token, "id": resumed["id"], "response": response });
    let with_binding = [("origin", ORIGIN), ("cookie", binding.as_str())];
    assert_eq!(
        call(
            &app,
            "/api/setup/finish",
            &with_binding,
            Some(finish(&superseded))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, signed_in, cookies) = call(
        &app,
        "/api/setup/finish",
        &with_binding,
        Some(finish(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{signed_in}");
    assert_eq!(signed_in["session"]["authentication_method"], "passkey");
    assert_eq!(signed_in["operator"]["login"], "first-admin");
    assert_eq!(signed_in["operator"]["platform_admin"], true);
    let session = named(&cookies, "__Host-midas-session").to_owned();
    assert_eq!(
        call(&app, "/api/session", &[("cookie", session.as_str())], None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "/api/setup/finish",
            &with_binding,
            Some(finish(&token))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "/api/setup/start", &origin, Some(start(&token)))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert!(matches!(
        setup::issue_token(database, Duration::from_secs(600)).await,
        Err(AuthError::Conflict)
    ));
    let used: i64 =
        sqlx::query_scalar("SELECT count(*) FROM midas.setup_tokens WHERE used_at IS NULL")
            .fetch_one(&mut *admin)
            .await
            .unwrap();
    assert_eq!(used, 0);
    let audit = Audit::new(database);
    let completed = audit
        .search_platform(&Filter {
            action: Some(Action::SetupComplete),
            ..Filter::default()
        })
        .await
        .unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0]
            .subject_operator_id
            .map(|id| id.to_string())
            .as_deref(),
        signed_in["session"]["operator_id"].as_str()
    );

    let tech = web
        .authentication()
        .create_operator("enroll-tech", "Új technikus")
        .await
        .unwrap();
    let codes = web
        .authentication()
        .replace_recovery_codes(tech)
        .await
        .unwrap();
    let sign_in = |code: &str| json!({ "login": "enroll-tech", "code": code });
    let (_, body, cookies) = call(
        &app,
        "/api/auth/recovery",
        &origin,
        Some(sign_in(&codes[0])),
    )
    .await;
    let old = named(&cookies, "__Host-midas-session").to_owned();
    let csrf = body["csrf_token"].as_str().unwrap().to_owned();
    let auth_headers = [
        ("origin", ORIGIN),
        ("cookie", old.as_str()),
        ("x-csrf-token", csrf.as_str()),
    ];
    let (status, issued, cookies) = call(
        &app,
        "/api/account/passkeys/start",
        &auth_headers,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{issued}");
    let binding = named(&cookies, "__Host-midas-login").to_owned();
    let mut tech_key = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let options: CreationChallengeResponse =
        serde_json::from_value(issued["options"].clone()).unwrap();
    let response = tech_key
        .do_registration(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();
    let both = format!("{old}; {binding}");
    let finish_headers = [
        ("origin", ORIGIN),
        ("cookie", both.as_str()),
        ("x-csrf-token", csrf.as_str()),
    ];
    let (status, enrolled, cookies) = call(
        &app,
        "/api/account/passkeys/finish",
        &finish_headers,
        Some(json!({ "id": issued["id"], "response": response })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{enrolled}");
    assert_eq!(enrolled["session"]["authentication_method"], "passkey");
    let new = named(&cookies, "__Host-midas-session").to_owned();
    assert_eq!(
        call(&app, "/api/session", &[("cookie", old.as_str())], None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "/api/session", &[("cookie", new.as_str())], None)
            .await
            .0,
        StatusCode::OK
    );
    let registered = audit
        .search_platform(&Filter {
            action: Some(Action::PasskeyRegister),
            subject: Some(tech),
            ..Filter::default()
        })
        .await
        .unwrap();
    assert_eq!(registered.len(), 1);

    let (_, body, cookies) = call(
        &app,
        "/api/auth/recovery",
        &origin,
        Some(sign_in(&codes[1])),
    )
    .await;
    let stale = named(&cookies, "__Host-midas-session").to_owned();
    let stale_csrf = body["csrf_token"].as_str().unwrap().to_owned();
    sqlx::query("UPDATE midas.browser_sessions SET authenticated_at = clock_timestamp() - interval '6 minutes' WHERE operator_id = $1")
        .bind(tech).execute(&mut *admin).await.unwrap();
    let (status, refused, _) = call(
        &app,
        "/api/account/passkeys/start",
        &[
            ("origin", ORIGIN),
            ("cookie", stale.as_str()),
            ("x-csrf-token", stale_csrf.as_str()),
        ],
        Some(json!({})),
    )
    .await;
    assert_eq!(
        (status, refused),
        (
            StatusCode::FORBIDDEN,
            json!({ "error": "recent_sign_in_required" })
        )
    );
    assert_eq!(
        call(
            &app,
            "/api/account/passkeys/start",
            &[("origin", ORIGIN), ("cookie", stale.as_str())],
            Some(json!({}))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
