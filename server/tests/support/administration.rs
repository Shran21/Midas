use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use midas_server::{
    administration::Administration,
    audit::{Actor, Audit, Filter},
    auth::{
        AuthConfig, BrowserBinding,
        http::{WebAuth, router},
        sessions::{SessionConfig, SessionToken},
    },
    authorization::{
        AccessError, Authorizer, BuiltinRole, Effect, Permission, RemoteMode, RemoteRequest, Rule,
        Scope, Target,
    },
    database::Database,
};
use serde_json::{Value, json};
use sqlx::PgConnection;
use tower::ServiceExt;
use uuid::Uuid;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::*;
use zeroize::Zeroizing;

const ORIGIN: &str = "https://midas.example";

#[derive(Clone)]
struct Client {
    cookie: String,
    csrf: String,
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    client: Option<&Client>,
    body: Option<Value>,
    extra: &[(&str, &str)],
) -> (StatusCode, Value, Option<String>) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(
            "198.51.100.93:40000".parse::<SocketAddr>().unwrap(),
        ));
    if method != "GET" {
        request = request.header("origin", ORIGIN);
    }
    if let Some(client) = client {
        request = request.header("cookie", &client.cookie);
        if method != "GET" {
            request = request.header("x-csrf-token", &client.csrf);
        }
    }
    for (name, value) in extra {
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
    let cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .find(|v| v.starts_with("__Host-midas-") && !v.ends_with('='));
    let bytes = to_bytes(response.into_body(), 200_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

async fn recovery(app: &Router, login: &str, code: &str) -> Client {
    let (status, body, cookie) = call(
        app,
        "POST",
        "/api/auth/recovery",
        None,
        Some(json!({ "login": login, "code": code })),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Client {
        cookie: cookie.unwrap(),
        csrf: body["csrf_token"].as_str().unwrap().to_owned(),
    }
}

async fn enroll(
    web: &WebAuth,
    login: &str,
    name: &str,
) -> (Uuid, WebauthnAuthenticator<SoftPasskey>) {
    let user = web
        .authentication()
        .create_operator(login, name)
        .await
        .unwrap();
    let binding = BrowserBinding::random().unwrap();
    let mut key = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let start = web
        .authentication()
        .begin_registration(user, &binding)
        .await
        .unwrap();
    let response = key
        .do_registration(Url::parse(ORIGIN).unwrap(), start.options)
        .unwrap();
    web.authentication()
        .finish_registration(&start.id, &binding, &response)
        .await
        .unwrap();
    (user, key)
}

async fn passkey(
    app: &Router,
    login: &str,
    key: &mut WebauthnAuthenticator<SoftPasskey>,
) -> Client {
    let (_, start, binding) = call(
        app,
        "POST",
        "/api/auth/passkey/start",
        None,
        Some(json!({ "login": login })),
        &[],
    )
    .await;
    let options: RequestChallengeResponse =
        serde_json::from_value(start["options"].clone()).unwrap();
    let response = key
        .do_authentication(Url::parse(ORIGIN).unwrap(), options)
        .unwrap();
    let binding = binding.unwrap();
    let (status, body, cookie) = call(
        app,
        "POST",
        "/api/auth/passkey/finish",
        None,
        Some(json!({ "id": start["id"], "response": response })),
        &[("cookie", &binding)],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Client {
        cookie: cookie.unwrap(),
        csrf: body["csrf_token"].as_str().unwrap().to_owned(),
    }
}

fn token(client: &Client) -> SessionToken {
    SessionToken::parse(client.cookie.split_once('=').unwrap().1).unwrap()
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
    let administration = Administration::new(database);
    let policy = Authorizer::new(database);
    let audit = Audit::new(database);
    let sessions = web.sessions();

    let (platform_admin, mut admin_key) = enroll(&web, "platform-admin", "Platform Admin").await;
    administration
        .set_platform_admin(platform_admin, true, Actor::SYSTEM)
        .await
        .unwrap();
    let codes = web
        .authentication()
        .replace_recovery_codes(platform_admin)
        .await
        .unwrap();
    let fresh = passkey(&app, "platform-admin", &mut admin_key).await;
    let fallback = recovery(&app, "platform-admin", &codes[0]).await;
    let (_, session, _) = call(&app, "GET", "/api/session", Some(&fresh), None, &[]).await;
    assert_eq!(session["operator"]["platform_admin"], true);

    let plain_user = web
        .authentication()
        .create_operator("plain-user", "Plain")
        .await
        .unwrap();
    let plain_codes = web
        .authentication()
        .replace_recovery_codes(plain_user)
        .await
        .unwrap();
    let plain = recovery(&app, "plain-user", &plain_codes[0]).await;
    let create = json!({ "name": "Gamma Kft." });
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/admin/tenants",
            Some(&plain),
            Some(create.clone()),
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, body, _) = call(
        &app,
        "POST",
        "/api/admin/tenants",
        Some(&fallback),
        Some(create.clone()),
        &[],
    )
    .await;
    assert_eq!(
        (status, body),
        (
            StatusCode::FORBIDDEN,
            json!({ "error": "fresh_passkey_required" })
        )
    );
    let no_csrf = Client {
        csrf: "invalid".into(),
        ..fresh.clone()
    };
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/admin/tenants",
            Some(&no_csrf),
            Some(create.clone()),
            &[]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/admin/tenants",
            Some(&fresh),
            Some(json!({ "name": "  " })),
            &[]
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (status, tenant, _) = call(
        &app,
        "POST",
        "/api/admin/tenants",
        Some(&fresh),
        Some(create),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{tenant}");
    let tenant: Uuid = tenant["id"].as_str().unwrap().parse().unwrap();
    let (_, context, _) = call(
        &app,
        "GET",
        &format!("/api/tenants/{tenant}"),
        Some(&fresh),
        None,
        &[],
    )
    .await;
    assert_eq!(
        context["capabilities"].as_array().unwrap().len(),
        Permission::ALL.len()
    );

    let mut sites = Vec::new();
    for name in ["Szeged", "Pécs"] {
        let (status, site, _) = call(
            &app,
            "POST",
            &format!("/api/tenants/{tenant}/sites"),
            Some(&fresh),
            Some(json!({ "name": name })),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        sites.push(site["id"].as_str().unwrap().parse::<Uuid>().unwrap());
    }
    let (_, listed, _) = call(
        &app,
        "GET",
        &format!("/api/tenants/{tenant}/sites"),
        Some(&fallback),
        None,
        &[],
    )
    .await;
    assert_eq!(listed.as_array().unwrap().len(), 2);
    let [north, south] = [sites[0], sites[1]];
    let [inside, outside] = [Uuid::new_v4(), Uuid::new_v4()];
    for (device, site) in [(inside, north), (outside, south)] {
        sqlx::query("INSERT INTO midas.devices (tenant_id,id,site_id,name,kind,platform) VALUES ($1,$2,$3,'PC','workstation','windows')")
            .bind(tenant).bind(device).bind(site).execute(&mut *admin).await.unwrap();
    }

    let new_operator = json!({ "login": "Gamma-Tech", "display_name": "Gamma technikus" });
    let (status, created, _) = call(
        &app,
        "POST",
        "/api/admin/operators",
        Some(&fresh),
        Some(new_operator.clone()),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["login"], "gamma-tech");
    let technician: Uuid = created["id"].as_str().unwrap().parse().unwrap();
    let initial: Vec<String> = serde_json::from_value(created["recovery_codes"].clone()).unwrap();
    assert_eq!(initial.len(), 10);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/admin/operators",
            Some(&fresh),
            Some(new_operator),
            &[]
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/tenants/{tenant}/members"),
            Some(&fresh),
            Some(json!({ "login": "nobody-here" })),
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/tenants/{tenant}/members"),
            Some(&fresh),
            Some(json!({ "login": "gamma-tech" })),
            &[]
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let scope = json!({ "kind": "site", "id": north });
    let (status, assigned, _) = call(
        &app,
        "POST",
        &format!("/api/tenants/{tenant}/assignments"),
        Some(&fresh),
        Some(
            json!({ "operator": technician, "role": BuiltinRole::Technician.id(), "scope": scope }),
        ),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{assigned}");
    let assignment = assigned["id"].as_str().unwrap().to_owned();
    let foreign_scope = json!({ "operator": technician, "role": BuiltinRole::Observer.id(), "scope": { "kind": "site", "id": Uuid::new_v4() } });
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/tenants/{tenant}/assignments"),
            Some(&fresh),
            Some(foreign_scope),
            &[]
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let tech = recovery(&app, "gamma-tech", &initial[0]).await;
    let (_, mine, _) = call(&app, "GET", "/api/tenants", Some(&tech), None, &[]).await;
    assert_eq!(mine, json!([{ "id": tenant, "name": "Gamma Kft." }]));
    let normal = RemoteRequest {
        mode: RemoteMode::Normal,
        uac: false,
        unattended: false,
    };
    let remote = |device| {
        let policy = &policy;
        let tech = token(&tech);
        async move {
            policy
                .check_remote_request(
                    sessions,
                    &tech,
                    Target {
                        tenant,
                        device: Some(device),
                    },
                    normal,
                )
                .await
        }
    };
    remote(inside).await.unwrap();
    assert_eq!(remote(outside).await.unwrap_err(), AccessError::Denied);
    let deny = json!({ "operator": technician, "permission": "remote.control", "effect": "deny", "scope": { "kind": "device", "id": inside } });
    let (status, rule, _) = call(
        &app,
        "POST",
        &format!("/api/tenants/{tenant}/overrides"),
        Some(&fresh),
        Some(deny),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(remote(inside).await.unwrap_err(), AccessError::Denied);
    let rule = rule["id"].as_str().unwrap().to_owned();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/tenants/{tenant}/overrides/{rule}"),
            Some(&fresh),
            None,
            &[]
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    remote(inside).await.unwrap();

    let (status, members, _) = call(
        &app,
        "GET",
        &format!("/api/tenants/{tenant}/members"),
        Some(&fallback),
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let member = members
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["login"] == "gamma-tech")
        .unwrap();
    assert_eq!(
        member["assignments"][0]["scope"],
        json!({ "kind": "site", "id": north })
    );
    assert_eq!(member["assignments"][0]["role_name"], "Technician");
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/tenants/{tenant}/members"),
            Some(&tech),
            None,
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let (manager, mut manager_key) = enroll(&web, "gamma-manager", "Gamma manager").await;
    let manager_client = passkey(&app, "gamma-manager", &mut manager_key).await;
    let limited = policy
        .create_role(
            "Gamma access managers",
            &[
                Rule {
                    permission: Permission::AccessManage,
                    effect: Effect::Allow,
                },
                Rule {
                    permission: Permission::DeviceRead,
                    effect: Effect::Allow,
                },
            ],
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    let readers = policy
        .create_role(
            "Gamma readers",
            &[Rule {
                permission: Permission::DeviceRead,
                effect: Effect::Allow,
            }],
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .set_membership(tenant, manager, true, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .assign_role(tenant, manager, limited, Scope::Tenant, Actor::SYSTEM)
        .await
        .unwrap();
    for (role, expected) in [
        (BuiltinRole::Administrator.id(), StatusCode::FORBIDDEN),
        (BuiltinRole::Observer.id(), StatusCode::FORBIDDEN),
        (readers, StatusCode::CREATED),
    ] {
        let (status, body, _) = call(
            &app,
            "POST",
            &format!("/api/tenants/{tenant}/assignments"),
            Some(&manager_client),
            Some(json!({ "operator": technician, "role": role, "scope": { "kind": "tenant" } })),
            &[],
        )
        .await;
        assert_eq!(status, expected, "{body}");
        if expected == StatusCode::FORBIDDEN {
            assert_eq!(body, json!({ "error": "escalation_denied" }));
        }
    }
    let grant = json!({ "operator": technician, "permission": "terminal.execute", "effect": "allow", "scope": { "kind": "tenant" } });
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/tenants/{tenant}/overrides"),
            Some(&manager_client),
            Some(grant),
            &[]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let admin_deny = json!({ "operator": technician, "permission": "remote.admin", "effect": "deny", "scope": { "kind": "tenant" } });
    let (_, denied, _) = call(
        &app,
        "POST",
        &format!("/api/tenants/{tenant}/overrides"),
        Some(&fresh),
        Some(admin_deny),
        &[],
    )
    .await;
    let denied = denied["id"].as_str().unwrap().to_owned();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/tenants/{tenant}/overrides/{denied}"),
            Some(&manager_client),
            None,
            &[]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let own_deny = json!({ "operator": technician, "permission": "files.write", "effect": "deny", "scope": { "kind": "tenant" } });
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/tenants/{tenant}/overrides"),
            Some(&manager_client),
            Some(own_deny),
            &[]
        )
        .await
        .0,
        StatusCode::CREATED
    );
    let other_tenant = policy.tenants(sessions, &token(&fresh)).await.unwrap();
    assert!(other_tenant.iter().all(|t| t.id == tenant));
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO midas.tenants (id,name) VALUES ($1,'Foreign')")
        .bind(foreign)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/tenants/{foreign}/members"),
            Some(&manager_client),
            None,
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/tenants/{foreign}/sites"),
            Some(&fresh),
            Some(json!({ "name": "X" })),
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/tenants/{tenant}/assignments/{assignment}"),
            Some(&fresh),
            None,
            &[]
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(remote(inside).await.unwrap_err(), AccessError::Denied);
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/tenants/{tenant}/assignments/{assignment}"),
            Some(&fresh),
            None,
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    policy
        .assign_role(
            tenant,
            technician,
            BuiltinRole::Technician.id(),
            Scope::Site(north),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/tenants/{tenant}/members/{technician}"),
            Some(&fresh),
            Some(json!({ "enabled": false })),
            &[]
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (_, mine, _) = call(&app, "GET", "/api/tenants", Some(&tech), None, &[]).await;
    assert_eq!(mine, json!([]));
    assert_eq!(remote(inside).await.unwrap_err(), AccessError::Denied);
    let (status, body, _) = call(
        &app,
        "PUT",
        &format!("/api/tenants/{tenant}/members/{technician}"),
        Some(&manager_client),
        Some(json!({ "enabled": true })),
        &[],
    )
    .await;
    assert_eq!(
        (status, body),
        (
            StatusCode::FORBIDDEN,
            json!({ "error": "escalation_denied" })
        )
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/tenants/{tenant}/members/{technician}"),
            Some(&fresh),
            Some(json!({ "enabled": true })),
            &[]
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    remote(inside).await.unwrap();

    let records = audit
        .search_tenant(
            &policy,
            sessions,
            &token(&fresh),
            tenant,
            &Filter {
                limit: 500,
                ..Filter::default()
            },
        )
        .await
        .unwrap();
    for action in [
        "tenant.create",
        "inventory.site_create",
        "access.membership_set",
        "access.role_assign",
        "access.role_unassign",
        "access.override_set",
        "access.override_remove",
    ] {
        assert!(records.iter().any(|r| r.action == action), "{action}");
    }
    for action in [
        "tenant.create",
        "inventory.site_create",
        "access.override_remove",
        "access.role_unassign",
    ] {
        assert!(
            records
                .iter()
                .filter(|r| r.action == action)
                .all(|r| r.actor_operator_id == Some(platform_admin)),
            "{action}"
        );
    }
    assert!(
        records
            .iter()
            .any(|r| r.action == "access.role_assign" && r.actor_operator_id == Some(manager))
    );
    let created = audit
        .search_platform(&Filter {
            subject: Some(technician),
            ..Filter::default()
        })
        .await
        .unwrap();
    assert!(created.iter().any(
        |r| r.action == "access.operator_create" && r.actor_operator_id == Some(platform_admin)
    ));
    let stored: Vec<String> = sqlx::query_scalar("SELECT row_to_json(a)::text FROM midas.platform_audit a UNION ALL SELECT row_to_json(a)::text FROM midas.tenant_audit a")
        .fetch_all(&mut *admin).await.unwrap();
    for code in &initial {
        assert!(stored.iter().all(|row| !row.contains(code.as_str())));
    }
}
