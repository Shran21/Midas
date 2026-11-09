use midas_server::database::driver as sqlx;
use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
    response::Response,
};
use futures_util::future::join_all;
use midas_server::{
    audit::{Action, Actor, Audit, Filter, Outcome},
    auth::{
        AuthConfig,
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
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;
use uuid::Uuid;
use webauthn_rs::prelude::Url;
use zeroize::Zeroizing;

const ORIGIN: &str = "https://midas.example";
const SOURCE: &str = "198.51.100.91";

async fn post(app: &Router, path: &str, headers: &[(&str, &str)], body: Value) -> Response {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("origin", ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo(
            format!("{SOURCE}:40000").parse::<SocketAddr>().unwrap(),
        ));
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    app.clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap()).unwrap()
}

fn session_cookie(response: &Response) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|v| {
            v.starts_with("__Host-midas-session=") && !v.starts_with("__Host-midas-session=;")
        })
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

fn filter(action: Action) -> Filter {
    Filter {
        action: Some(action),
        ..Filter::default()
    }
}

async fn sign_in(app: &Router, login: &str, code: &str) -> (String, Value) {
    let response = post(
        app,
        "/api/auth/recovery",
        &[],
        json!({ "login": login, "code": code }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = session_cookie(&response);
    (cookie, json_body(response).await)
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
    let audit = Audit::new(database);
    let policy = Authorizer::new(database);
    let auth = web.authentication();
    let user = auth.create_operator("audit-user", "Auditor").await.unwrap();
    let codes = auth.replace_recovery_codes(user).await.unwrap();
    let start = audit.verify_platform().await.unwrap();
    assert_eq!(start.first_invalid, None);

    let (cookie, signed_in) = sign_in(&app, "audit-user", &codes[0]).await;
    let token = SessionToken::parse(cookie.split_once('=').unwrap().1).unwrap();
    let sessions = web.sessions();
    let session = sessions.validate(&token, false).await.unwrap();
    let wrong = "a1b2c3d4".repeat(8);
    for login in ["audit-user", "audit-nobody"] {
        let response = post(
            &app,
            "/api/auth/recovery",
            &[],
            json!({ "login": login, "code": wrong }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let malformed = post(
        &app,
        "/api/auth/recovery",
        &[],
        json!({ "login": "audit-user", "code": wrong, "extra": true }),
    )
    .await;
    assert!(malformed.status().is_client_error() && malformed.status() != StatusCode::UNAUTHORIZED);

    let after = Filter {
        limit: 500,
        ..filter(Action::SignIn)
    };
    let sign_ins: Vec<_> = audit
        .search_platform(&after)
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.sequence > start.length)
        .collect();
    assert_eq!(sign_ins.len(), 3, "{sign_ins:?}");
    let success = sign_ins.iter().find(|r| r.outcome == "success").unwrap();
    assert_eq!(success.subject_operator_id, Some(user));
    assert_eq!(success.actor_operator_id, Some(user));
    assert_eq!(success.actor_session_id, Some(session.id));
    assert_eq!(success.source.as_deref(), Some(SOURCE));
    assert_eq!(success.details, json!({ "method": "recovery_code" }));
    let failures: Vec<_> = sign_ins.iter().filter(|r| r.outcome == "failure").collect();
    assert_eq!(failures.len(), 2);
    assert_eq!(
        failures
            .iter()
            .filter(|r| r.subject_operator_id == Some(user))
            .count(),
        1
    );
    assert!(
        failures
            .iter()
            .all(|r| r.details == json!({ "method": "recovery_code" }))
    );
    assert!(failures.iter().all(|r| r.actor_operator_id.is_none()));
    let by_subject = audit
        .search_platform(&Filter {
            subject: Some(user),
            outcome: Some(Outcome::Failure),
            ..Filter::default()
        })
        .await
        .unwrap();
    assert_eq!(by_subject.len(), 1);
    let page = audit
        .search_platform(&Filter {
            limit: 1,
            ..filter(Action::SignIn)
        })
        .await
        .unwrap();
    let older = audit
        .search_platform(&Filter {
            limit: 1,
            before_sequence: Some(page[0].sequence),
            ..filter(Action::SignIn)
        })
        .await
        .unwrap();
    assert!(older[0].sequence < page[0].sequence);

    let tenant = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    for id in [tenant, foreign] {
        sqlx::query("INSERT INTO midas.tenants (id,name) VALUES ($1,'Audit fixture')")
            .bind(id)
            .execute(&mut *admin)
            .await
            .unwrap();
    }
    let device = Uuid::new_v4();
    sqlx::query("INSERT INTO midas.devices (tenant_id,id,name,kind,platform) VALUES ($1,$2,'Audited','workstation','windows')")
        .bind(tenant).bind(device).execute(&mut *admin).await.unwrap();
    let actor = Actor::session(&session);
    let search = |tenant, filter: Filter| {
        let audit = &audit;
        let policy = &policy;
        let token = &token;
        async move {
            audit
                .search_tenant(policy, sessions, token, tenant, &filter)
                .await
        }
    };
    policy
        .set_membership(tenant, user, true, actor)
        .await
        .unwrap();
    assert_eq!(
        search(tenant, Filter::default()).await.unwrap_err(),
        AccessError::Denied
    );
    let auditor = policy
        .create_role(
            "Audit readers",
            &[Rule {
                permission: Permission::AuditRead,
                effect: Effect::Allow,
            }],
            actor,
        )
        .await
        .unwrap();
    policy
        .assign_role(tenant, user, auditor, Scope::Tenant, actor)
        .await
        .unwrap();
    let technician = policy
        .assign_role(
            tenant,
            user,
            BuiltinRole::Technician.id(),
            Scope::Device(device),
            actor,
        )
        .await
        .unwrap();
    let denied = policy
        .override_permission(
            tenant,
            user,
            Scope::Device(device),
            Rule {
                permission: Permission::FilesRead,
                effect: Effect::Deny,
            },
            actor,
        )
        .await
        .unwrap();
    assert!(policy.remove_override(tenant, denied, actor).await.unwrap());
    assert!(!policy.remove_override(tenant, denied, actor).await.unwrap());
    let changes = search(tenant, Filter::default()).await.unwrap();
    let actions: Vec<_> = changes.iter().rev().map(|r| r.action.as_str()).collect();
    assert_eq!(
        actions,
        [
            "access.membership_set",
            "access.role_assign",
            "access.role_assign",
            "access.override_set",
            "access.override_remove",
        ]
    );
    assert!(changes.iter().all(|r| r.actor_operator_id == Some(user)
        && r.actor_session_id == Some(session.id)
        && r.subject_operator_id == Some(user)
        && r.tenant_id == Some(tenant)));
    let device_scoped = search(
        tenant,
        Filter {
            device: Some(device),
            ..Filter::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(device_scoped.len(), 3);
    assert_eq!(
        device_scoped.last().unwrap().details["scope"],
        json!({ "kind": "device", "id": device.to_string() })
    );
    let created = audit
        .search_platform(&filter(Action::RoleCreate))
        .await
        .unwrap();
    assert_eq!(created[0].details["name"], "Audit readers");
    assert_eq!(created[0].actor_operator_id, Some(user));

    let normal = RemoteRequest {
        mode: RemoteMode::Normal,
        uac: false,
        unattended: false,
    };
    let target = Target {
        tenant,
        device: Some(device),
    };
    policy
        .check_remote_request(sessions, &token, target, normal)
        .await
        .unwrap();
    let uac = RemoteRequest {
        uac: true,
        ..normal
    };
    assert_eq!(
        policy
            .check_remote_request(sessions, &token, target, uac)
            .await
            .unwrap_err(),
        AccessError::Denied
    );
    let decisions = search(tenant, filter(Action::RemoteRequest)).await.unwrap();
    assert_eq!(decisions.len(), 2);
    assert_eq!(decisions[0].outcome, "denied");
    assert_eq!(
        decisions[0].details,
        json!({ "mode": "normal", "uac": true, "unattended": false, "result": "denied" })
    );
    assert_eq!(decisions[1].outcome, "success");
    assert_eq!(decisions[1].device_id, Some(device));
    for unknown in [foreign, foreign, Uuid::new_v4()] {
        assert_eq!(
            policy
                .check_remote_request(
                    sessions,
                    &token,
                    Target {
                        tenant: unknown,
                        device: Some(device)
                    },
                    normal
                )
                .await
                .unwrap_err(),
            AccessError::Denied
        );
    }
    let foreign_decisions: Vec<(Uuid, String)> = sqlx::query_as("SELECT tenant_id, outcome FROM midas.tenant_audit WHERE tenant_id = $1 AND action = 'remote.request'")
        .bind(foreign).fetch_all(&mut *admin).await.unwrap();
    assert_eq!(foreign_decisions, vec![(foreign, "denied".to_owned()); 2]);
    let unattributed = audit
        .search_platform(&filter(Action::RemoteRequest))
        .await
        .unwrap();
    assert_eq!(unattributed.len(), 1);
    assert!(
        unattributed[0]
            .tenant_id
            .is_some_and(|t| t != tenant && t != foreign)
    );
    assert_eq!(
        search(foreign, Filter::default()).await.unwrap_err(),
        AccessError::Denied
    );

    let mut tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('midas.tenant_id',$1,true)")
        .bind(foreign.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let visible: Vec<Uuid> = sqlx::query_scalar("SELECT tenant_id FROM midas.tenant_audit")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert!(!visible.is_empty() && visible.iter().all(|t| *t == foreign));
    let forged = sqlx::query("INSERT INTO midas.tenant_audit (tenant_id, action, outcome, details) VALUES ($1, 'auth.sign_in', 'success', '{}')")
        .bind(tenant).execute(&mut *tx).await.unwrap_err();
    assert_eq!(forged.as_database_error().unwrap().code().unwrap(), "42501");
    tx.rollback().await.unwrap();
    let chosen = sqlx::query("INSERT INTO midas.platform_audit (sequence, action, outcome, details) VALUES (1, 'auth.sign_in', 'success', '{}')")
        .execute(runtime).await.unwrap_err();
    assert_eq!(chosen.as_database_error().unwrap().code().unwrap(), "42501");
    for statement in [
        "UPDATE midas.platform_audit SET outcome = 'success'",
        "DELETE FROM midas.platform_audit",
        "TRUNCATE midas.platform_audit",
        "TRUNCATE midas.tenant_audit",
    ] {
        let error = sqlx::query(statement).execute(runtime).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().unwrap(), "42501");
        let error = sqlx::query(statement)
            .execute(&mut *admin)
            .await
            .unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().unwrap(), "42501");
    }

    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT row_to_json(a)::text FROM midas.platform_audit a UNION ALL SELECT row_to_json(a)::text FROM midas.tenant_audit a",
    )
    .fetch_all(&mut *admin)
    .await
    .unwrap();
    let secrets = [
        codes[0].to_string(),
        codes[1].to_string(),
        wrong.clone(),
        token.encode().to_string(),
        token.csrf().unwrap().to_string(),
        signed_in["csrf_token"].as_str().unwrap().to_owned(),
        "audit-nobody".to_owned(),
    ];
    for row in &stored {
        for secret in &secrets {
            assert!(
                !row.contains(secret.as_str()),
                "credential material in audit row"
            );
        }
    }

    let concurrent = join_all((0..24).map(|i| {
        let policy = &policy;
        async move {
            policy
                .set_membership(tenant, user, true, Actor::SYSTEM)
                .await
                .map(|_| i)
        }
    }))
    .await;
    assert!(concurrent.iter().all(Result::is_ok));
    let chain = audit
        .verify_tenant(&policy, sessions, &token, tenant)
        .await
        .unwrap();
    assert_eq!(chain.first_invalid, None);
    assert!(chain.length >= 31);
    let platform = audit.verify_platform().await.unwrap();
    assert_eq!(platform.first_invalid, None);
    assert!(platform.length > start.length);

    sqlx::query("REVOKE INSERT ON midas.tenant_audit FROM midas_runtime")
        .execute(&mut *admin)
        .await
        .unwrap();
    assert_eq!(
        policy
            .check_remote_request(sessions, &token, target, normal)
            .await
            .unwrap_err(),
        AccessError::Unavailable
    );
    assert!(
        policy
            .remove_assignment(tenant, technician, actor)
            .await
            .is_err()
    );
    sqlx::query("GRANT INSERT (tenant_id, action, outcome, actor_operator_id, actor_session_id, subject_operator_id, device_id, source, details) ON midas.tenant_audit TO midas_runtime")
        .execute(&mut *admin).await.unwrap();
    policy
        .check_remote_request(sessions, &token, target, normal)
        .await
        .unwrap();
    sqlx::query("REVOKE INSERT ON midas.platform_audit FROM midas_runtime")
        .execute(&mut *admin)
        .await
        .unwrap();
    let count = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM midas.browser_sessions WHERE operator_id = $1",
        )
        .bind(user)
        .fetch_one(runtime)
        .await
        .unwrap()
    };
    let before = count().await;
    let unaudited = post(
        &app,
        "/api/auth/recovery",
        &[],
        json!({ "login": "audit-user", "code": codes[1].as_str() }),
    )
    .await;
    assert_eq!(unaudited.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(count().await, before);
    sqlx::query("GRANT INSERT (tenant_id, action, outcome, actor_operator_id, actor_session_id, subject_operator_id, device_id, source, details) ON midas.platform_audit TO midas_runtime")
        .execute(&mut *admin).await.unwrap();

    let csrf = token.csrf().unwrap();
    let logout = post(
        &app,
        "/api/session/logout",
        &[("cookie", &cookie), ("x-csrf-token", csrf.as_str())],
        json!({}),
    )
    .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    let signed_out = audit
        .search_platform(&Filter {
            actor: Some(user),
            ..filter(Action::SignOut)
        })
        .await
        .unwrap();
    assert_eq!(signed_out.len(), 1);
    assert_eq!(signed_out[0].actor_session_id, Some(session.id));

    sqlx::query("ALTER TABLE midas.tenant_audit DISABLE TRIGGER audit_immutable")
        .execute(&mut *admin)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE midas.tenant_audit SET outcome = 'denied' WHERE tenant_id = $1 AND sequence = 3",
    )
    .bind(tenant)
    .execute(&mut *admin)
    .await
    .unwrap();
    let tampered: (i64, Option<i64>) = sqlx::query_as("SELECT count(*), (SELECT min(sequence) FROM midas.tenant_audit a WHERE a.tenant_id = $1 AND a.hash <> midas.audit_hash(a.previous_hash, a.sequence, a.occurred_at, a.tenant_id, a.action, a.outcome, a.actor_operator_id, a.actor_session_id, a.subject_operator_id, a.device_id, a.source, a.details)) FROM midas.tenant_audit WHERE tenant_id = $1")
        .bind(tenant).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(tampered.1, Some(3));
    sqlx::query("DELETE FROM midas.tenant_audit WHERE tenant_id = $1 AND sequence = 1")
        .bind(foreign)
        .execute(&mut *admin)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE midas.tenant_audit ENABLE TRIGGER audit_immutable")
        .execute(&mut *admin)
        .await
        .unwrap();
    let (cookie, _) = sign_in(&app, "audit-user", &codes[2]).await;
    let token = SessionToken::parse(cookie.split_once('=').unwrap().1).unwrap();
    let broken = audit
        .verify_tenant(&policy, sessions, &token, tenant)
        .await
        .unwrap();
    assert_eq!(broken.first_invalid, Some(3));
    assert_eq!(broken.length, chain.length + 1);
    policy
        .set_membership(foreign, user, true, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .assign_role(foreign, user, auditor, Scope::Tenant, Actor::SYSTEM)
        .await
        .unwrap();
    let gap = audit
        .verify_tenant(&policy, sessions, &token, foreign)
        .await
        .unwrap();
    assert_eq!(gap.first_invalid, Some(2));
}
