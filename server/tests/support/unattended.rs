use std::sync::Arc;

use axum::http::StatusCode;
use midas_server::{
    audit::Actor,
    auth::{
        AuthConfig,
        http::{WebAuth, router},
        sessions::{SessionConfig, SessionToken},
    },
    authorization::{
        AccessError, Authorizer, BuiltinRole, Effect, Permission, RemoteMode, Rule, Scope, Target,
        unattended::{
            Presence, SessionObservation, SessionSelection, UnattendedDecision, UnattendedRequest,
        },
    },
    database::{Database, driver as sqlx},
    enrollment::{Authority, Enrollment},
};
use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;
use webauthn_rs::prelude::Url;
use zeroize::Zeroizing;

use super::administration::{ORIGIN, call, enroll, passkey};

pub async fn verify(database: &Database, admin: &mut PgConnection) {
    let mut key = [0; 32];
    getrandom::fill(&mut key).unwrap();
    let web = Arc::new(
        WebAuth::new(
            database,
            AuthConfig::new(Url::parse(ORIGIN).unwrap(), Zeroizing::new(key)),
            SessionConfig::default(),
            4,
        )
        .await
        .unwrap(),
    );
    let app = router(web.clone());
    let policy = Authorizer::new(database);
    let (operator, mut authenticator) =
        enroll(&web, "unattended-operator", "Unattended operator").await;
    let client = passkey(&app, "unattended-operator", &mut authenticator).await;
    let token = SessionToken::parse(client.cookie().split_once('=').unwrap().1).unwrap();
    let tenant = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    let server = Uuid::new_v4();
    let workstation = Uuid::new_v4();
    for id in [tenant, foreign] {
        sqlx::query("INSERT INTO midas.tenants (id,name) VALUES ($1,'Unattended fixture')")
            .bind(id)
            .execute(&mut *admin)
            .await
            .unwrap();
    }
    for (owner, id, kind) in [
        (tenant, server, "server"),
        (tenant, workstation, "workstation"),
        (foreign, server, "server"),
    ] {
        sqlx::query("INSERT INTO midas.devices (tenant_id,id,name,kind,platform,enrollment) VALUES ($1,$2,'Target',$3,'windows','active')")
            .bind(owner).bind(id).bind(kind).execute(&mut *admin).await.unwrap();
    }
    policy
        .set_membership(tenant, operator, true, Actor::SYSTEM)
        .await
        .unwrap();
    let tenant_path = format!("/api/tenants/{tenant}/unattended-policy");
    let device_path = format!("/api/tenants/{tenant}/devices/{server}/unattended-policy");
    assert_eq!(
        call(
            &app,
            "PUT",
            &tenant_path,
            None,
            Some(json!({"enabled":true})),
            &[]
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&app, "GET", &tenant_path, Some(&client), None, &[])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let role = policy
        .assign_role(
            tenant,
            operator,
            BuiltinRole::Administrator.id(),
            Scope::Tenant,
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    let (status, body, _) = call(&app, "GET", &tenant_path, Some(&client), None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    let (status, body, _) = call(&app, "GET", &device_path, Some(&client), None, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled"], false);
    assert_eq!(body["tenant_enabled"], false);
    assert_eq!(body["reboot_confirmation_required"], true);
    assert_eq!(body["reconnect_verification_required"], true);
    assert!(body["maintenance_window"].is_null());
    assert_eq!(
        call(
            &app,
            "PUT",
            &tenant_path,
            None,
            Some(json!({"enabled":true})),
            &[("cookie", client.cookie()), ("x-csrf-token", "wrong")]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/tenants/{foreign}/unattended-policy"),
            Some(&client),
            Some(json!({"enabled":true})),
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/tenants/{foreign}/devices/{server}/unattended-policy"),
            Some(&client),
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
            "GET",
            &format!("/api/tenants/{tenant}/devices/{workstation}/unattended-policy"),
            Some(&client),
            None,
            &[]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let setting =
        json!({"enabled":true,"maintenance_window":{"starts_at":now-60,"ends_at":now+3600}});
    for value in [
        json!({"enabled":true}),
        json!({"enabled":true,"maintenance_window":{"starts_at":now+1,"ends_at":now}}),
        json!({"enabled":true,"maintenance_window":{"starts_at":0,"ends_at":1}}),
    ] {
        assert_eq!(
            call(&app, "PUT", &device_path, Some(&client), Some(value), &[])
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let denied = policy
        .override_permission(
            tenant,
            operator,
            Scope::Device(server),
            Rule {
                permission: Permission::RemoteUnattended,
                effect: Effect::Deny,
            },
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    for (path, value) in [
        (&tenant_path, json!({"enabled":true})),
        (&device_path, setting.clone()),
    ] {
        let (status, body, _) = call(&app, "PUT", path, Some(&client), Some(value), &[]).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["error"], "escalation_denied");
    }
    policy
        .remove_override(tenant, denied, Actor::SYSTEM)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &device_path,
            Some(&client),
            Some(setting.clone()),
            &[]
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let body = call(&app, "GET", &device_path, Some(&client), None, &[])
        .await
        .1;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["tenant_enabled"], false);
    assert_eq!(body["within_maintenance_window"], true);

    let ca_key = KeyPair::generate().unwrap();
    let mut ca = CertificateParams::default();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = ca.self_signed(&ca_key).unwrap();
    let authority = Authority::from_pem(&ca.pem(), &ca_key.serialize_pem()).unwrap();
    let agent_key = KeyPair::generate().unwrap();
    let csr = CertificateParams::default()
        .serialize_request(&agent_key)
        .unwrap();
    let certificate = authority.issue(csr.der(), tenant, server).unwrap();
    sqlx::query("INSERT INTO midas.agent_credentials (fingerprint,tenant_id,device_id,expires_at) VALUES ($1,$2,$3,clock_timestamp()+interval '1 hour')")
        .bind(&certificate.fingerprint).bind(tenant).bind(server).execute(&mut *admin).await.unwrap();
    let agents = Enrollment::new(database, authority);
    let admission = agents.admit(&certificate.der).await.unwrap();
    let selected = SessionSelection {
        connection_id: admission.connection_id(),
        os_session_id: "console:1".into(),
        generation: Uuid::new_v4(),
    };
    let vacant =
        SessionObservation::from_agent(&admission, selected.clone(), Presence::Vacant).unwrap();
    let occupied =
        SessionObservation::from_agent(&admission, selected.clone(), Presence::Occupied).unwrap();
    let unknown =
        SessionObservation::from_agent(&admission, selected.clone(), Presence::Unknown).unwrap();
    let mut request = UnattendedRequest {
        mode: RemoteMode::Administrative,
        uac: true,
        reason: "Planned maintenance".into(),
        session: selected.clone(),
    };
    let target = Target {
        tenant,
        device: Some(server),
    };
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
            .await,
        Err(AccessError::Denied)
    ));
    assert_eq!(
        call(
            &app,
            "PUT",
            &tenant_path,
            Some(&client),
            Some(json!({"enabled":true})),
            &[]
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let decision = policy
        .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
        .await
        .unwrap();
    match decision {
        UnattendedDecision::Eligible {
            access,
            session,
            expires_at,
        } => {
            assert_eq!(access.operator_id, operator);
            assert_eq!(access.tenant_id, tenant);
            assert_eq!(access.device_id, Some(server));
            assert!(access.permissions.contains(&Permission::RemoteAdmin));
            assert!(access.permissions.contains(&Permission::RemoteUacInteract));
            assert_eq!(session, selected);
            let current = time::OffsetDateTime::now_utc().unix_timestamp();
            assert!(expires_at > current && expires_at <= current + 30);
        }
        other => panic!("{other:?}"),
    }
    for permission in [
        Permission::RemoteAdmin,
        Permission::RemoteUacInteract,
        Permission::RemoteControl,
    ] {
        let denied = policy
            .override_permission(
                tenant,
                operator,
                Scope::Device(server),
                Rule {
                    permission,
                    effect: Effect::Deny,
                },
                Actor::SYSTEM,
            )
            .await
            .unwrap();
        assert!(matches!(
            policy
                .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
                .await,
            Err(AccessError::Denied)
        ));
        policy
            .remove_override(tenant, denied, Actor::SYSTEM)
            .await
            .unwrap();
    }
    request.mode = RemoteMode::Normal;
    let decision = policy
        .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
        .await
        .unwrap();
    match decision {
        UnattendedDecision::Eligible { access, .. } => {
            assert!(!access.permissions.contains(&Permission::RemoteAdmin));
            assert!(access.permissions.contains(&Permission::RemoteUacInteract));
        }
        other => panic!("{other:?}"),
    }
    request.mode = RemoteMode::Administrative;
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&occupied))
            .await
            .unwrap(),
        UnattendedDecision::LocalConsentRequired
    ));
    for observation in [None, Some(&unknown)] {
        assert!(matches!(
            policy
                .check_unattended_request(web.sessions(), &token, target, &request, observation)
                .await,
            Err(AccessError::Offline)
        ));
    }
    for reason in [
        "".to_owned(),
        "   ".into(),
        "Bad\nreason".into(),
        "x".repeat(513),
    ] {
        request.reason = reason;
        assert!(matches!(
            policy
                .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
                .await,
            Err(AccessError::InvalidInput)
        ));
    }
    request.reason = "Planned maintenance".into();
    request.session.generation = Uuid::new_v4();
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
            .await,
        Err(AccessError::Denied)
    ));
    request.session = selected.clone();
    let reconnected = agents.admit(&certificate.der).await.unwrap();
    assert_ne!(reconnected.connection_id(), admission.connection_id());
    assert!(
        SessionObservation::from_agent(&reconnected, selected.clone(), Presence::Vacant).is_err()
    );
    for statement in [
        "UPDATE midas.agent_credentials SET revoked_at=clock_timestamp() WHERE fingerprint=$1",
        "UPDATE midas.agent_credentials SET expires_at=clock_timestamp()-interval '1 second' WHERE fingerprint=$1",
    ] {
        sqlx::query(statement)
            .bind(&certificate.fingerprint)
            .execute(&mut *admin)
            .await
            .unwrap();
        assert!(matches!(
            policy
                .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
                .await,
            Err(AccessError::Offline)
        ));
        sqlx::query("UPDATE midas.agent_credentials SET revoked_at=NULL,expires_at=clock_timestamp()+interval '1 hour' WHERE fingerprint=$1")
            .bind(&certificate.fingerprint).execute(&mut *admin).await.unwrap();
    }
    assert_eq!(call(&app, "PUT", &device_path, Some(&client), Some(json!({"enabled":true,"maintenance_window":{"starts_at":now+3600,"ends_at":now+7200}})), &[]).await.0, StatusCode::NO_CONTENT);
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
            .await,
        Err(AccessError::Denied)
    ));
    assert_eq!(
        call(
            &app,
            "PUT",
            &device_path,
            Some(&client),
            Some(setting.clone()),
            &[]
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let denied = policy
        .override_permission(
            tenant,
            operator,
            Scope::Device(server),
            Rule {
                permission: Permission::RemoteUnattended,
                effect: Effect::Deny,
            },
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
            .await,
        Err(AccessError::Denied)
    ));
    assert_eq!(
        call(
            &app,
            "PUT",
            &device_path,
            Some(&client),
            Some(json!({"enabled":false})),
            &[]
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    policy
        .remove_override(tenant, denied, Actor::SYSTEM)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "PUT", &device_path, Some(&client), Some(setting), &[])
            .await
            .0,
        StatusCode::NO_CONTENT
    );

    let audit: String = sqlx::query_scalar("SELECT details::text FROM midas.tenant_audit WHERE tenant_id=$1 AND action='remote.request' AND outcome='success' ORDER BY sequence DESC LIMIT 1")
        .bind(tenant).fetch_one(&mut *admin).await.unwrap();
    let audit: serde_json::Value = serde_json::from_str(&audit).unwrap();
    assert_eq!(audit["reason"], "Planned maintenance");
    assert_eq!(
        audit["session"]["connection_id"],
        selected.connection_id.to_string()
    );
    assert!(audit["expires_at"].is_i64());
    request.reason = "\"".repeat(512);
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
            .await
            .unwrap(),
        UnattendedDecision::Eligible { .. }
    ));
    request.reason = "Planned maintenance".into();
    sqlx::raw_sql("CREATE FUNCTION midas.fail_unattended_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action IN ('remote.unattended_policy_set','remote.request') THEN RAISE EXCEPTION 'test audit failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER fail_unattended_audit BEFORE INSERT ON midas.tenant_audit FOR EACH ROW EXECUTE FUNCTION midas.fail_unattended_audit();")
        .execute(&mut *admin).await.unwrap();
    assert_eq!(
        call(
            &app,
            "PUT",
            &tenant_path,
            Some(&client),
            Some(json!({"enabled":false})),
            &[]
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(&app, "GET", &tenant_path, Some(&client), None, &[])
            .await
            .1["enabled"],
        true
    );
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
            .await,
        Err(AccessError::Unavailable)
    ));
    sqlx::raw_sql("DROP TRIGGER fail_unattended_audit ON midas.tenant_audit; DROP FUNCTION midas.fail_unattended_audit();").execute(&mut *admin).await.unwrap();
    sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp()-interval '6 minutes' WHERE operator_id=$1").bind(operator).execute(&mut *admin).await.unwrap();
    assert_eq!(
        call(&app, "GET", &tenant_path, Some(&client), None, &[])
            .await
            .0,
        StatusCode::OK
    );
    let (status, body, _) = call(
        &app,
        "PUT",
        &tenant_path,
        Some(&client),
        Some(json!({"enabled":false})),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "fresh_passkey_required");
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
            .await,
        Err(AccessError::FreshPasskeyRequired)
    ));
    policy
        .remove_assignment(tenant, role, Actor::SYSTEM)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "GET", &tenant_path, Some(&client), None, &[])
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert!(matches!(
        policy
            .check_unattended_request(web.sessions(), &token, target, &request, Some(&vacant))
            .await,
        Err(AccessError::Denied)
    ));
}
