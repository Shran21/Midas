use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use midas_server::{
    audit::Actor,
    auth::{
        AuthConfig,
        http::{WebAuth, router},
        sessions::SessionConfig,
    },
    authorization::{Authorizer, BuiltinRole, Effect, Permission, Rule, Scope},
    database::Database,
};
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;
use uuid::Uuid;
use webauthn_rs::prelude::Url;
use zeroize::Zeroizing;

const ORIGIN: &str = "https://midas.example";

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(
            "198.51.100.92:40000".parse::<SocketAddr>().unwrap(),
        ));
    if method != "GET" {
        request = request
            .header("origin", ORIGIN)
            .header(header::CONTENT_TYPE, "application/json");
    }
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .find(|v| v.starts_with("__Host-midas-session=") && v.len() > 22);
    let bytes = to_bytes(response.into_body(), 100_000).await.unwrap();
    let mut value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if let Some(cookie) = cookie {
        value["cookie"] = Value::String(cookie);
    }
    (status, value)
}

async fn sign_in(app: &Router, login: &str, code: &str) -> (String, Value) {
    let (status, body) = call(
        app,
        "POST",
        "/api/auth/recovery",
        None,
        Some(json!({ "login": login, "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    (body["cookie"].as_str().unwrap().to_owned(), body)
}

pub async fn verify(database: &Database, admin: &mut PgConnection, runtime: &PgPool) {
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
    let policy = Authorizer::new(database);
    let auth = web.authentication();
    let user = auth
        .create_operator("tenant-user", "Ügyfélkezelő")
        .await
        .unwrap();
    let other = auth
        .create_operator("tenant-other", "Másik kezelő")
        .await
        .unwrap();
    let codes = auth.replace_recovery_codes(user).await.unwrap();
    let other_codes = auth.replace_recovery_codes(other).await.unwrap();

    let [alpha, beta, disabled, foreign] = [(); 4].map(|()| Uuid::new_v4());
    for (id, name) in [
        (alpha, "Alfa Kft."),
        (beta, "béta Zrt."),
        (disabled, "Letiltott Bt."),
        (foreign, "Idegen Kft."),
    ] {
        sqlx::query("INSERT INTO midas.tenants (id,name) VALUES ($1,$2)")
            .bind(id)
            .bind(name)
            .execute(&mut *admin)
            .await
            .unwrap();
    }
    let site = Uuid::new_v4();
    sqlx::query("INSERT INTO midas.sites (tenant_id,id,name) VALUES ($1,$2,'Központ')")
        .bind(beta)
        .bind(site)
        .execute(&mut *admin)
        .await
        .unwrap();
    for tenant in [alpha, beta, disabled] {
        policy
            .set_membership(tenant, user, true, Actor::SYSTEM)
            .await
            .unwrap();
    }
    policy
        .set_membership(disabled, user, false, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .set_membership(foreign, other, true, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .assign_role(
            alpha,
            user,
            BuiltinRole::Observer.id(),
            Scope::Tenant,
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .assign_role(
            beta,
            user,
            BuiltinRole::Technician.id(),
            Scope::Site(site),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .override_permission(
            beta,
            user,
            Scope::Tenant,
            Rule {
                permission: Permission::RemoteControl,
                effect: Effect::Deny,
            },
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .override_permission(
            beta,
            user,
            Scope::Site(site),
            Rule {
                permission: Permission::MonitorRead,
                effect: Effect::Deny,
            },
            Actor::SYSTEM,
        )
        .await
        .unwrap();

    assert_eq!(
        call(&app, "GET", "/api/tenants", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (cookie, signed_in) = sign_in(&app, "tenant-user", &codes[0]).await;
    assert_eq!(
        signed_in["operator"],
        json!({ "login": "tenant-user", "display_name": "Ügyfélkezelő" })
    );
    let (status, session) = call(&app, "GET", "/api/session", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["operator"]["display_name"], "Ügyfélkezelő");

    let (status, list) = call(&app, "GET", "/api/tenants", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        list,
        json!([{ "id": alpha.to_string(), "name": "Alfa Kft." }, { "id": beta.to_string(), "name": "béta Zrt." }])
    );
    let (status, context) = call(
        &app,
        "GET",
        &format!("/api/tenants/{alpha}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        context,
        json!({ "id": alpha.to_string(), "name": "Alfa Kft.", "capabilities": ["device.read", "monitor.read"] })
    );
    let (_, context) = call(
        &app,
        "GET",
        &format!("/api/tenants/{beta}"),
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(
        context["capabilities"],
        json!(["device.read", "monitor.read", "remote.view"])
    );
    for missing in [disabled, foreign, Uuid::new_v4()] {
        let (status, body) = call(
            &app,
            "GET",
            &format!("/api/tenants/{missing}"),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, json!({ "error": "tenant_not_found" }));
    }
    assert_eq!(
        call(&app, "GET", "/api/tenants/not-a-uuid", Some(&cookie), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );

    let (other_cookie, _) = sign_in(&app, "tenant-other", &other_codes[0]).await;
    let (_, other_list) = call(&app, "GET", "/api/tenants", Some(&other_cookie), None).await;
    assert_eq!(
        other_list,
        json!([{ "id": foreign.to_string(), "name": "Idegen Kft." }])
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/tenants/{alpha}"),
            Some(&other_cookie),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    policy
        .set_membership(beta, user, false, Actor::SYSTEM)
        .await
        .unwrap();
    let (_, list) = call(&app, "GET", "/api/tenants", Some(&cookie), None).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/tenants/{beta}"),
            Some(&cookie),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let mut tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('midas.operator_id',$1,true)")
        .bind(user.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let visible: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM midas.tenants ORDER BY name")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(visible, [alpha]);
    let memberships: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenant_memberships")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(memberships, 3);
    for statement in [
        "SELECT count(*) FROM midas.sites",
        "SELECT count(*) FROM midas.role_assignments",
        "SELECT count(*) FROM midas.tenant_audit",
    ] {
        let count: i64 = sqlx::query_scalar(statement)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(count, 0, "{statement}");
    }
    let write = sqlx::query(
        "INSERT INTO midas.tenant_memberships (tenant_id, operator_id) VALUES ($1, $2)",
    )
    .bind(foreign)
    .bind(user)
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(write.as_database_error().unwrap().code().unwrap(), "42501");
    tx.rollback().await.unwrap();

    let csrf = session["csrf_token"].as_str().unwrap().to_owned();
    let request = Request::builder()
        .method("POST")
        .uri("/api/session/logout")
        .header("origin", ORIGIN)
        .header("cookie", &cookie)
        .header("x-csrf-token", csrf)
        .extension(ConnectInfo(
            "198.51.100.92:40000".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/api/tenants", Some(&cookie), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
