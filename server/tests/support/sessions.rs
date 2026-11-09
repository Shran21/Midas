use midas_server::database::driver as sqlx;
use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use midas_server::{
    auth::{
        AuthConfig, AuthError, BrowserBinding,
        http::{WebAuth, router},
        sessions::{IssuedSession, SessionConfig, SessionToken, Sessions},
    },
    database::Database,
};
use serde_json::{Value, json};
use sqlx::PgConnection;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tower::ServiceExt;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::*;
use zeroize::Zeroizing;

const ORIGIN: &str = "https://midas.example";
const COOKIE: &str = "__Host-midas-session";

fn config() -> AuthConfig {
    let mut key = [0; 32];
    getrandom::fill(&mut key).unwrap();
    let mut config = AuthConfig::new(Url::parse(ORIGIN).unwrap(), Zeroizing::new(key));
    config.account_attempts = 300;
    config.source_attempts = 1000;
    config
}

async fn issue(web: &WebAuth, login: &str, code: &str) -> IssuedSession {
    let proof = web
        .authentication()
        .login_recovery(
            login,
            Zeroizing::new(code.to_owned()),
            "192.0.2.71".parse().unwrap(),
        )
        .await
        .unwrap();
    web.sessions().issue(proof, None).await.unwrap()
}

fn cookie_value(token: &SessionToken) -> String {
    format!("{COOKIE}={}", *token.encode())
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(
            "198.51.100.71:40000".parse::<SocketAddr>().unwrap(),
        ));
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let body = if let Some(body) = body {
        request = request.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    app.clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap()
}

async fn body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap()
}

fn received_cookie(response: &Response, name: &str) -> String {
    let prefix = format!("{name}=");
    let cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|v| v.starts_with(&prefix))
        .unwrap();
    for flag in [
        "Path=/",
        "Secure",
        "HttpOnly",
        "SameSite=Strict",
        "Max-Age=",
    ] {
        assert!(cookie.contains(flag));
    }
    assert!(!cookie.contains("Domain="));
    cookie.split(';').next().unwrap().to_owned()
}

pub async fn verify(database: &Database, admin: &mut PgConnection) {
    let web = Arc::new(
        WebAuth::new(database, config(), SessionConfig::default(), 8)
            .await
            .unwrap(),
    );
    let app = router(web.clone());
    let user = web
        .authentication()
        .create_operator("browser", "Böngésző")
        .await
        .unwrap();
    let other = web
        .authentication()
        .create_operator("browser-other", "Másik")
        .await
        .unwrap();
    let codes = web
        .authentication()
        .replace_recovery_codes(user)
        .await
        .unwrap();
    let other_codes = web
        .authentication()
        .replace_recovery_codes(other)
        .await
        .unwrap();
    let initial = issue(&web, "browser", &codes[0]).await;
    let second = issue(&web, "browser", &codes[1]).await;
    let other_session = issue(&web, "browser-other", &other_codes[0]).await;
    let cookie = cookie_value(&initial.token);
    let csrf = initial.token.csrf().unwrap();
    let info = request(&app, "GET", "/api/session", &[("cookie", &cookie)], None).await;
    assert_eq!(info.status(), StatusCode::OK);
    assert_eq!(info.headers()[header::CACHE_CONTROL], "no-store");
    let info = body(info).await;
    assert_eq!(info["csrf_token"], csrf.as_str());
    assert_eq!(info["session"]["operator_id"], user.to_string());
    assert_eq!(info["session"]["authentication_method"], "recovery_code");
    assert!(!info.to_string().contains(initial.token.encode().as_str()));
    let repeated =
        body(request(&app, "GET", "/api/session", &[("cookie", &cookie)], None).await).await;
    assert_eq!(repeated["csrf_token"], info["csrf_token"]);
    let sessions =
        body(request(&app, "GET", "/api/sessions", &[("cookie", &cookie)], None).await).await;
    assert_eq!(sessions.as_array().unwrap().len(), 2);
    assert_eq!(
        request(&app, "GET", "/api/session", &[], None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/session?token=ignored",
            &[("cookie", &cookie)],
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let duplicated = format!("{cookie}; {cookie}");
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/session",
            &[("cookie", &duplicated)],
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    for headers in [
        vec![("cookie", cookie.as_str()), ("origin", ORIGIN)],
        vec![("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())],
        vec![
            ("cookie", cookie.as_str()),
            ("origin", "https://evil.example"),
            ("x-csrf-token", csrf.as_str()),
        ],
        vec![
            ("cookie", cookie.as_str()),
            ("origin", ORIGIN),
            ("x-csrf-token", "wrong"),
        ],
    ] {
        assert_eq!(
            request(&app, "POST", "/api/session/logout", &headers, None)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        web.sessions()
            .validate(&initial.token, false)
            .await
            .unwrap();
    }
    let second_csrf = second.token.csrf().unwrap();
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/session/logout",
            &[
                ("cookie", &cookie),
                ("origin", ORIGIN),
                ("x-csrf-token", &second_csrf)
            ],
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let authorized = [
        ("cookie", cookie.as_str()),
        ("origin", ORIGIN),
        ("x-csrf-token", csrf.as_str()),
    ];
    assert_eq!(
        request(
            &app,
            "DELETE",
            &format!("/api/sessions/{}", other_session.info.id),
            &authorized,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    web.sessions()
        .validate(&other_session.token, false)
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            "DELETE",
            &format!("/api/sessions/{}", second.info.id),
            &authorized,
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(web.sessions().validate(&second.token, false).await.is_err());
    let persisted = Sessions::new(database, SessionConfig::default()).unwrap();
    persisted.validate(&initial.token, false).await.unwrap();
    assert!(persisted.validate(&second.token, false).await.is_err());
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT token_hash FROM midas.browser_sessions WHERE id = $1")
            .bind(initial.info.id)
            .fetch_one(&mut *admin)
            .await
            .unwrap();
    assert_eq!(stored.len(), 32);
    assert_ne!(hex::encode(&stored), initial.token.encode().as_str());

    let response = request(
        &app,
        "POST",
        "/api/auth/recovery",
        &[("origin", ORIGIN), ("cookie", &cookie)],
        Some(json!({"login":"browser","code":codes[2].as_str()})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let current_cookie = received_cookie(&response, COOKIE);
    let current_token = SessionToken::parse(current_cookie.split_once('=').unwrap().1).unwrap();
    assert_ne!(
        current_token.encode().as_str(),
        initial.token.encode().as_str()
    );
    assert!(
        web.sessions()
            .validate(&initial.token, false)
            .await
            .is_err()
    );
    assert!(persisted.validate(&initial.token, false).await.is_err());
    let current_info = body(response).await;
    assert_eq!(
        current_info["csrf_token"],
        current_token.csrf().unwrap().as_str()
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/auth/recovery",
            &[("origin", ORIGIN)],
            Some(json!({"login":"browser","code":codes[2].as_str()}))
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(request(&app, "POST", "/api/auth/password", &[("origin", ORIGIN)], Some(json!({"login":"unknown-browser","password":"long invalid password","code":"123456"}))).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/auth/recovery",
            &[("origin", "https://evil.example")],
            Some(json!({"login":"browser","code":codes[3].as_str()}))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/auth/password",
            &[("origin", ORIGIN)],
            None
        )
        .await
        .status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let huge = "x".repeat(70_000);
    assert_eq!(
        request(
            &app,
            "POST",
            "/api/auth/password",
            &[("origin", ORIGIN)],
            Some(json!({"login":"browser","password":huge,"code":"123456"}))
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );

    let absolute = issue(&web, "browser", &codes[3]).await;
    sqlx::query("UPDATE midas.browser_sessions SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1").bind(absolute.info.id).execute(&mut *admin).await.unwrap();
    assert!(matches!(
        web.sessions().validate(&absolute.token, false).await,
        Err(AuthError::InvalidCredentials)
    ));
    let idle = issue(&web, "browser", &codes[4]).await;
    sqlx::query("UPDATE midas.browser_sessions SET idle_expires_at = clock_timestamp() - interval '1 second' WHERE id = $1").bind(idle.info.id).execute(&mut *admin).await.unwrap();
    assert!(web.sessions().validate(&idle.token, true).await.is_err());
    sqlx::query("UPDATE midas.browser_sessions SET last_seen_at = clock_timestamp() - interval '2 minutes' WHERE id = $1").bind(other_session.info.id).execute(&mut *admin).await.unwrap();
    let last: f64 = sqlx::query_scalar("SELECT extract(epoch FROM last_seen_at)::double precision FROM midas.browser_sessions WHERE id=$1").bind(other_session.info.id).fetch_one(&mut *admin).await.unwrap();
    persisted
        .validate(&other_session.token, false)
        .await
        .unwrap();
    let untouched: f64 = sqlx::query_scalar("SELECT extract(epoch FROM last_seen_at)::double precision FROM midas.browser_sessions WHERE id=$1").bind(other_session.info.id).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(last, untouched);
    persisted
        .validate(&other_session.token, true)
        .await
        .unwrap();
    let touched: f64 = sqlx::query_scalar("SELECT extract(epoch FROM last_seen_at)::double precision FROM midas.browser_sessions WHERE id=$1").bind(other_session.info.id).fetch_one(&mut *admin).await.unwrap();
    assert!(touched > last);

    sockets(&app, &web, &current_token, &other_session.token, admin).await;
    assert_eq!(
        request(
            &app,
            "GET",
            "/api/session",
            &[("cookie", &current_cookie)],
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let disabled = issue(&web, "browser", &codes[5]).await;
    sqlx::query("UPDATE midas.operators SET enabled = false WHERE id = $1")
        .bind(user)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert!(
        web.sessions()
            .validate(&disabled.token, false)
            .await
            .is_err()
    );
    sqlx::query("UPDATE midas.operators SET enabled = true WHERE id = $1")
        .bind(user)
        .execute(&mut *admin)
        .await
        .unwrap();
    let proof = web
        .authentication()
        .login_recovery("browser", codes[6].clone(), "192.0.2.71".parse().unwrap())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE midas.operators SET credential_version = credential_version + 1 WHERE id = $1",
    )
    .bind(user)
    .execute(&mut *admin)
    .await
    .unwrap();
    assert!(web.sessions().issue(proof, None).await.is_err());
    assert!(
        web.sessions()
            .validate(&disabled.token, false)
            .await
            .is_err()
    );

    passkey_http(&web, &app).await;
    password_http(&web, &app, admin).await;
    let mut limited = config();
    limited.source_attempts = 2;
    let limited = Arc::new(
        WebAuth::new(database, limited, SessionConfig::default(), 1)
            .await
            .unwrap(),
    );
    let app = router(limited);
    for index in 0..3 {
        let req = Request::builder()
            .method("POST")
            .uri("/api/auth/passkey/start")
            .extension(ConnectInfo(
                "198.51.100.245:40000".parse::<SocketAddr>().unwrap(),
            ))
            .header("origin", ORIGIN)
            .header("content-type", "application/json")
            .header("x-forwarded-for", format!("203.0.113.{}", index + 1))
            .body(Body::from(
                json!({"login":format!("unknown-limit-{index}")}).to_string(),
            ))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            response.status(),
            if index < 2 {
                StatusCode::OK
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
    }
}

async fn passkey_http(web: &WebAuth, app: &Router) {
    let user = web
        .authentication()
        .create_operator("browser-passkey", "Passkey")
        .await
        .unwrap();
    let binding = BrowserBinding::random().unwrap();
    let mut token = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let start = web
        .authentication()
        .begin_registration(user, &binding)
        .await
        .unwrap();
    let response = token
        .do_registration(Url::parse(ORIGIN).unwrap(), start.options)
        .unwrap();
    web.authentication()
        .finish_registration(&start.id, &binding, &response)
        .await
        .unwrap();
    let start = request(
        app,
        "POST",
        "/api/auth/passkey/start",
        &[("origin", ORIGIN)],
        Some(json!({"login":"browser-passkey"})),
    )
    .await;
    assert_eq!(start.status(), StatusCode::OK);
    let binding_cookie = received_cookie(&start, "__Host-midas-login");
    let start = body(start).await;
    let options: RequestChallengeResponse =
        serde_json::from_value(start["options"].clone()).unwrap();
    let response = token
        .do_authentication(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();
    let payload = json!({"id":start["id"], "response":response});
    assert_eq!(
        request(
            app,
            "POST",
            "/api/auth/passkey/finish",
            &[("origin", ORIGIN)],
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let success = request(
        app,
        "POST",
        "/api/auth/passkey/finish",
        &[("origin", ORIGIN), ("cookie", &binding_cookie)],
        Some(payload.clone()),
    )
    .await;
    assert_eq!(success.status(), StatusCode::OK);
    received_cookie(&success, COOKIE);
    assert_eq!(
        body(success).await["session"]["authentication_method"],
        "passkey"
    );
    assert_eq!(
        request(
            app,
            "POST",
            "/api/auth/passkey/finish",
            &[("origin", ORIGIN), ("cookie", &binding_cookie)],
            Some(payload)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

async fn password_http(web: &WebAuth, app: &Router, admin: &mut PgConnection) {
    let user = web
        .authentication()
        .create_operator("browser-password", "TOTP")
        .await
        .unwrap();
    let enrollment = web
        .authentication()
        .begin_fallback_enrollment(user)
        .await
        .unwrap();
    let uri = Url::parse(&enrollment.provisioning_uri().unwrap()).unwrap();
    let secret = uri
        .query_pairs()
        .find(|(key, _)| key == "secret")
        .unwrap()
        .1
        .into_owned();
    let totp = totp_rs::Builder::new()
        .with_secret(totp_rs::Secret::try_from_base32(&secret).unwrap())
        .build()
        .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let code = totp.generate(now).to_string();
    let password = "A browser password for testing!";
    web.authentication()
        .finish_fallback_enrollment(enrollment, Zeroizing::new(password.into()), &code)
        .await
        .unwrap();
    sqlx::query("UPDATE midas.operators SET totp_last_step = -1 WHERE id = $1")
        .bind(user)
        .execute(&mut *admin)
        .await
        .unwrap();
    let payload = json!({"login":"browser-password", "password":password,"code":code});
    let success = request(
        app,
        "POST",
        "/api/auth/password",
        &[("origin", ORIGIN)],
        Some(payload.clone()),
    )
    .await;
    assert_eq!(success.status(), StatusCode::OK);
    received_cookie(&success, COOKIE);
    assert_eq!(
        body(success).await["session"]["authentication_method"],
        "password_totp"
    );
    assert_eq!(
        request(
            app,
            "POST",
            "/api/auth/password",
            &[("origin", ORIGIN)],
            Some(payload)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

async fn sockets(
    app: &Router,
    web: &Arc<WebAuth>,
    token: &SessionToken,
    external: &SessionToken,
    admin: &mut PgConnection,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let url = format!("ws://{address}/api/session/socket");
    let make = |token: &SessionToken, origin: &str| {
        let mut request = url.clone().into_client_request().unwrap();
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
        request
            .headers_mut()
            .insert("cookie", cookie_value(token).parse().unwrap());
        request
    };
    assert!(
        connect_async(make(token, "https://evil.example"))
            .await
            .is_err()
    );
    let mut missing_origin = make(token, ORIGIN);
    missing_origin.headers_mut().remove("origin");
    assert!(connect_async(missing_origin).await.is_err());
    let (mut wrong, _) = connect_async(make(token, ORIGIN)).await.unwrap();
    wrong
        .send(Message::Text(
            json!({"csrf_token":"wrong"}).to_string().into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), wrong.next())
            .await
            .unwrap(),
        Some(Ok(Message::Close(_)))
    ));
    let (mut socket, _) = connect_async(make(token, ORIGIN)).await.unwrap();
    socket
        .send(Message::Text(
            json!({"csrf_token":token.csrf().unwrap().as_str()})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let welcome = tokio::time::timeout(Duration::from_secs(3), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(welcome.is_text());
    socket
        .send(Message::Text(json!({"type":"session"}).to_string().into()))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .is_text()
    );
    let cookie = cookie_value(token);
    let csrf = token.csrf().unwrap();
    let response = request(
        app,
        "POST",
        "/api/session/logout",
        &[
            ("cookie", &cookie),
            ("origin", ORIGIN),
            ("x-csrf-token", &csrf),
        ],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .all(|c| c.to_str().unwrap().contains("Max-Age=0"))
    );
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap(),
        Some(Ok(Message::Close(_)))
    ));
    assert!(connect_async(make(token, ORIGIN)).await.is_err());
    assert!(web.sessions().validate(token, false).await.is_err());
    let (mut external_socket, _) = connect_async(make(external, ORIGIN)).await.unwrap();
    external_socket
        .send(Message::Text(
            json!({"csrf_token":external.csrf().unwrap().as_str()})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), external_socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .is_text()
    );
    let id = web.sessions().validate(external, false).await.unwrap().id;
    sqlx::query("DELETE FROM midas.browser_sessions WHERE id = $1")
        .bind(id)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(17), external_socket.next())
            .await
            .unwrap(),
        Some(Ok(Message::Close(_)))
    ));
    server.abort();
    let _ = server.await;
}
