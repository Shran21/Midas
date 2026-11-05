use std::net::IpAddr;

use midas_server::{
    audit::Actor,
    auth::{
        AuthConfig, AuthService, BrowserBinding,
        sessions::{SessionConfig, Sessions},
    },
    authorization::{
        AccessError, Authorizer, BuiltinRole, Effect, Permission, RemoteMode, RemoteRequest, Rule,
        Scope, Target,
    },
    database::Database,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;
use webauthn_authenticator_rs::{WebauthnAuthenticator, softpasskey::SoftPasskey};
use webauthn_rs::prelude::*;
use zeroize::Zeroizing;

fn allow(permission: Permission) -> Rule {
    Rule {
        permission,
        effect: Effect::Allow,
    }
}
fn deny(permission: Permission) -> Rule {
    Rule {
        permission,
        effect: Effect::Deny,
    }
}
fn normal() -> RemoteRequest {
    RemoteRequest {
        mode: RemoteMode::Normal,
        uac: false,
        unattended: false,
    }
}

pub async fn verify(database: &Database, admin: &mut PgConnection, runtime: &PgPool) {
    let origin = Url::parse("https://midas.example").unwrap();
    let mut key = [0; 32];
    getrandom::fill(&mut key).unwrap();
    let auth = AuthService::new(
        database,
        AuthConfig::new(origin.clone(), Zeroizing::new(key)),
    )
    .await
    .unwrap();
    let sessions = Sessions::new(database, SessionConfig::default()).unwrap();
    let policy = Authorizer::new(database);
    let user = auth
        .create_operator("access-operator", "Operátor")
        .await
        .unwrap();
    let scoped_user = auth
        .create_operator("access-scoped", "Szűkített")
        .await
        .unwrap();
    let binding = BrowserBinding::random().unwrap();
    let mut authenticator = WebauthnAuthenticator::new(SoftPasskey::new(true));
    let registration = auth.begin_registration(user, &binding).await.unwrap();
    let response = authenticator
        .do_registration(origin.clone(), registration.options)
        .unwrap();
    auth.finish_registration(&registration.id, &binding, &response)
        .await
        .unwrap();
    let codes = auth.replace_recovery_codes(user).await.unwrap();
    let source: IpAddr = "192.0.2.82".parse().unwrap();
    let challenge = auth
        .begin_passkey_login("access-operator", source, &binding)
        .await
        .unwrap();
    let response = authenticator
        .do_authentication(origin, challenge.options)
        .unwrap();
    let passkey = sessions
        .issue(
            auth.finish_passkey_login(&challenge.id, source, &binding, &response)
                .await
                .unwrap(),
            None,
        )
        .await
        .unwrap();
    let fallback = sessions
        .issue(
            auth.login_recovery("access-operator", codes[0].clone(), source)
                .await
                .unwrap(),
            None,
        )
        .await
        .unwrap();
    let scoped_codes = auth.replace_recovery_codes(scoped_user).await.unwrap();
    let scoped = sessions
        .issue(
            auth.login_recovery("access-scoped", scoped_codes[0].clone(), source)
                .await
                .unwrap(),
            None,
        )
        .await
        .unwrap();
    let tenant = Uuid::new_v4();
    let foreign_tenant = Uuid::new_v4();
    for id in [tenant, foreign_tenant] {
        sqlx::query("INSERT INTO midas.tenants (id,name) VALUES ($1,'Authorization fixture')")
            .bind(id)
            .execute(&mut *admin)
            .await
            .unwrap();
    }
    let site = Uuid::new_v4();
    let other_site = Uuid::new_v4();
    let foreign_site = Uuid::new_v4();
    for (owner, id) in [
        (tenant, site),
        (tenant, other_site),
        (foreign_tenant, foreign_site),
    ] {
        sqlx::query("INSERT INTO midas.sites (tenant_id,id,name) VALUES ($1,$2,'Site')")
            .bind(owner)
            .bind(id)
            .execute(&mut *admin)
            .await
            .unwrap();
    }
    let workstation = Uuid::new_v4();
    let server = Uuid::new_v4();
    let outside = Uuid::new_v4();
    let linux = Uuid::new_v4();
    for (id, location, kind, platform) in [
        (workstation, site, "workstation", "windows"),
        (server, site, "server", "windows"),
        (outside, other_site, "workstation", "windows"),
        (linux, site, "server", "linux"),
    ] {
        sqlx::query("INSERT INTO midas.devices (tenant_id,id,site_id,name,kind,platform) VALUES ($1,$2,$3,'Device',$4,$5)").bind(tenant).bind(id).bind(location).bind(kind).bind(platform).execute(&mut *admin).await.unwrap();
    }
    sqlx::query("INSERT INTO midas.devices (tenant_id,id,site_id,name,kind,platform) VALUES ($1,$2,$3,'Same ID, different tenant','server','windows')").bind(foreign_tenant).bind(server).bind(foreign_site).execute(&mut *admin).await.unwrap();
    let group = Uuid::new_v4();
    sqlx::query("INSERT INTO midas.device_groups (tenant_id,id,name) VALUES ($1,$2,'Group')")
        .bind(tenant)
        .bind(group)
        .execute(&mut *admin)
        .await
        .unwrap();
    for device in [workstation, server] {
        sqlx::query("INSERT INTO midas.device_group_members (tenant_id,group_id,device_id) VALUES ($1,$2,$3)").bind(tenant).bind(group).bind(device).execute(&mut *admin).await.unwrap();
    }
    let target = |device| Target {
        tenant,
        device: Some(device),
    };
    let check = |result: Result<_, AccessError>| {
        assert!(matches!(result, Err(AccessError::Denied)), "{result:?}");
    };
    check(
        policy
            .authorize(
                &sessions,
                &passkey.token,
                target(workstation),
                Permission::DeviceRead,
            )
            .await,
    );
    policy
        .set_membership(tenant, user, true, Actor::SYSTEM)
        .await
        .unwrap();
    check(
        policy
            .authorize(
                &sessions,
                &passkey.token,
                target(workstation),
                Permission::DeviceRead,
            )
            .await,
    );
    policy
        .assign_role(
            tenant,
            user,
            BuiltinRole::Observer.id(),
            Scope::Tenant,
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .authorize(
            &sessions,
            &fallback.token,
            target(workstation),
            Permission::DeviceRead,
        )
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(workstation), normal())
            .await,
    );
    policy
        .assign_role(
            tenant,
            user,
            BuiltinRole::Technician.id(),
            Scope::Tenant,
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .check_remote_request(&sessions, &fallback.token, target(workstation), normal())
        .await
        .unwrap();
    let uac = RemoteRequest {
        uac: true,
        ..normal()
    };
    let administrative = RemoteRequest {
        mode: RemoteMode::Administrative,
        ..normal()
    };
    let unattended = RemoteRequest {
        unattended: true,
        ..administrative
    };
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(workstation), uac)
            .await,
    );
    check(
        policy
            .check_remote_request(
                &sessions,
                &passkey.token,
                target(workstation),
                administrative,
            )
            .await,
    );
    check(
        policy
            .authorize(
                &sessions,
                &passkey.token,
                target(workstation),
                Permission::RemoteUnattended,
            )
            .await,
    );
    policy
        .override_permission(
            tenant,
            user,
            Scope::Device(workstation),
            allow(Permission::RemoteUacInteract),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .check_remote_request(&sessions, &passkey.token, target(workstation), uac)
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(
                &sessions,
                &passkey.token,
                target(workstation),
                administrative,
            )
            .await,
    );
    assert!(matches!(
        policy
            .check_remote_request(&sessions, &fallback.token, target(workstation), uac)
            .await,
        Err(AccessError::FreshPasskeyRequired)
    ));
    policy
        .override_permission(
            tenant,
            user,
            Scope::Device(workstation),
            allow(Permission::RemoteAdmin),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .check_remote_request(
            &sessions,
            &passkey.token,
            target(workstation),
            administrative,
        )
        .await
        .unwrap();
    let denied_uac = policy
        .override_permission(
            tenant,
            user,
            Scope::Group(group),
            deny(Permission::RemoteUacInteract),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(workstation), uac)
            .await,
    );
    policy
        .check_remote_request(
            &sessions,
            &passkey.token,
            target(workstation),
            administrative,
        )
        .await
        .unwrap();
    assert!(
        !policy
            .remove_override(foreign_tenant, denied_uac, Actor::SYSTEM)
            .await
            .unwrap()
    );
    assert!(
        policy
            .remove_override(tenant, denied_uac, Actor::SYSTEM)
            .await
            .unwrap()
    );
    policy
        .check_remote_request(&sessions, &passkey.token, target(workstation), uac)
        .await
        .unwrap();

    let admin_role = policy
        .assign_role(
            tenant,
            user,
            BuiltinRole::Administrator.id(),
            Scope::Tenant,
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    let catalog: Vec<String> =
        sqlx::query_scalar("SELECT name FROM midas.permissions ORDER BY name")
            .fetch_all(runtime)
            .await
            .unwrap();
    let mut known: Vec<_> = Permission::ALL
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    known.sort();
    assert_eq!(catalog, known);
    for permission in Permission::ALL {
        if *permission == Permission::RemoteUnattended {
            check(
                policy
                    .authorize(&sessions, &passkey.token, target(server), *permission)
                    .await,
            );
            continue;
        }
        policy
            .authorize(&sessions, &passkey.token, target(server), *permission)
            .await
            .unwrap();
        if permission.requires_device() {
            check(
                policy
                    .authorize(
                        &sessions,
                        &passkey.token,
                        Target {
                            tenant,
                            device: None,
                        },
                        *permission,
                    )
                    .await,
            );
        }
        if permission.requires_fresh_passkey() {
            assert!(matches!(
                policy
                    .authorize(&sessions, &fallback.token, target(server), *permission)
                    .await,
                Err(AccessError::FreshPasskeyRequired)
            ));
        }
    }
    check(
        policy
            .authorize(
                &sessions,
                &passkey.token,
                Target {
                    tenant: foreign_tenant,
                    device: Some(server),
                },
                Permission::DeviceRead,
            )
            .await,
    );
    check(
        policy
            .authorize(
                &sessions,
                &passkey.token,
                target(Uuid::new_v4()),
                Permission::DeviceRead,
            )
            .await,
    );
    check(
        policy
            .check_remote_request(
                &sessions,
                &passkey.token,
                Target {
                    tenant,
                    device: None,
                },
                normal(),
            )
            .await,
    );
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(linux), uac)
            .await,
    );
    let denied_control = policy
        .override_permission(
            tenant,
            user,
            Scope::Tenant,
            deny(Permission::RemoteControl),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .override_permission(
            tenant,
            user,
            Scope::Device(workstation),
            allow(Permission::RemoteControl),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(workstation), normal())
            .await,
    );
    policy
        .remove_override(tenant, denied_control, Actor::SYSTEM)
        .await
        .unwrap();
    let restrictive = policy
        .create_role(
            "No input at this site",
            &[deny(Permission::RemoteControl)],
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    let restriction = policy
        .assign_role(tenant, user, restrictive, Scope::Site(site), Actor::SYSTEM)
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(workstation), normal())
            .await,
    );
    policy
        .check_remote_request(&sessions, &passkey.token, target(outside), normal())
        .await
        .unwrap();
    let tenant_wide = Target {
        tenant,
        device: None,
    };
    let denied_files = policy
        .override_permission(
            tenant,
            user,
            Scope::Device(outside),
            deny(Permission::FilesWrite),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    check(
        policy
            .authorize(
                &sessions,
                &passkey.token,
                tenant_wide,
                Permission::FilesWrite,
            )
            .await,
    );
    policy
        .authorize(
            &sessions,
            &passkey.token,
            target(workstation),
            Permission::FilesWrite,
        )
        .await
        .unwrap();
    policy
        .remove_override(tenant, denied_files, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .authorize(
            &sessions,
            &passkey.token,
            tenant_wide,
            Permission::FilesWrite,
        )
        .await
        .unwrap();
    sqlx::query("UPDATE midas.roles SET enabled=false WHERE id=$1")
        .bind(restrictive)
        .execute(runtime)
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(workstation), normal())
            .await,
    );
    policy
        .remove_assignment(tenant, restriction, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .check_remote_request(&sessions, &passkey.token, target(workstation), normal())
        .await
        .unwrap();

    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(server), unattended)
            .await,
    );
    sqlx::query("UPDATE midas.tenants SET unattended_enabled=true WHERE id=$1")
        .bind(tenant)
        .execute(&mut *admin)
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(server), unattended)
            .await,
    );
    sqlx::query(
        "UPDATE midas.devices SET unattended_enabled=true WHERE tenant_id=$1 AND id IN ($2,$3)",
    )
    .bind(tenant)
    .bind(server)
    .bind(workstation)
    .execute(&mut *admin)
    .await
    .unwrap();
    policy
        .check_remote_request(&sessions, &passkey.token, target(server), unattended)
        .await
        .unwrap();
    assert!(matches!(
        policy
            .check_remote_request(&sessions, &fallback.token, target(server), unattended)
            .await,
        Err(AccessError::FreshPasskeyRequired)
    ));
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(workstation), unattended)
            .await,
    );
    policy
        .authorize(
            &sessions,
            &passkey.token,
            target(server),
            Permission::RemoteUnattended,
        )
        .await
        .unwrap();
    for (device, permission) in [
        (workstation, Permission::RemoteUnattended),
        (linux, Permission::RemoteUacInteract),
    ] {
        check(
            policy
                .authorize(&sessions, &passkey.token, target(device), permission)
                .await,
        );
    }
    sqlx::query("UPDATE midas.tenants SET unattended_enabled=false WHERE id=$1")
        .bind(tenant)
        .execute(&mut *admin)
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(server), unattended)
            .await,
    );
    sqlx::query("UPDATE midas.tenants SET unattended_enabled=true WHERE id=$1")
        .bind(tenant)
        .execute(&mut *admin)
        .await
        .unwrap();
    let unattended_denied = policy
        .override_permission(
            tenant,
            user,
            Scope::Device(server),
            deny(Permission::RemoteUnattended),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &passkey.token, target(server), unattended)
            .await,
    );
    policy
        .remove_override(tenant, unattended_denied, Actor::SYSTEM)
        .await
        .unwrap();
    let uac_and_admin = RemoteRequest {
        uac: true,
        ..unattended
    };
    policy
        .check_remote_request(&sessions, &passkey.token, target(server), uac_and_admin)
        .await
        .unwrap();

    policy
        .set_membership(tenant, scoped_user, true, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .set_membership(foreign_tenant, scoped_user, true, Actor::SYSTEM)
        .await
        .unwrap();
    let scoped_assignment = policy
        .assign_role(
            tenant,
            scoped_user,
            BuiltinRole::Technician.id(),
            Scope::Site(other_site),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .check_remote_request(&sessions, &scoped.token, target(outside), normal())
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &scoped.token, target(server), normal())
            .await,
    );
    check(
        policy
            .authorize(
                &sessions,
                &scoped.token,
                Target {
                    tenant,
                    device: None,
                },
                Permission::DeviceRead,
            )
            .await,
    );
    let group_assignment = policy
        .assign_role(
            tenant,
            scoped_user,
            BuiltinRole::Technician.id(),
            Scope::Group(group),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .check_remote_request(&sessions, &scoped.token, target(server), normal())
        .await
        .unwrap();
    sqlx::query("DELETE FROM midas.device_group_members WHERE tenant_id=$1 AND group_id=$2 AND device_id=$3").bind(tenant).bind(group).bind(server).execute(&mut *admin).await.unwrap();
    check(
        policy
            .check_remote_request(&sessions, &scoped.token, target(server), normal())
            .await,
    );
    let device_assignment = policy
        .assign_role(
            tenant,
            scoped_user,
            BuiltinRole::Technician.id(),
            Scope::Device(server),
            Actor::SYSTEM,
        )
        .await
        .unwrap();
    policy
        .check_remote_request(&sessions, &scoped.token, target(server), normal())
        .await
        .unwrap();
    assert!(
        !policy
            .remove_assignment(foreign_tenant, device_assignment, Actor::SYSTEM)
            .await
            .unwrap()
    );
    policy
        .remove_assignment(tenant, device_assignment, Actor::SYSTEM)
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &scoped.token, target(server), normal())
            .await,
    );
    policy
        .remove_assignment(tenant, scoped_assignment, Actor::SYSTEM)
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &scoped.token, target(outside), normal())
            .await,
    );
    policy
        .remove_assignment(tenant, group_assignment, Actor::SYSTEM)
        .await
        .unwrap();
    check(
        policy
            .check_remote_request(&sessions, &scoped.token, target(workstation), normal())
            .await,
    );
    assert!(matches!(
        policy
            .assign_role(
                tenant,
                scoped_user,
                BuiltinRole::Observer.id(),
                Scope::Site(foreign_site),
                Actor::SYSTEM
            )
            .await,
        Err(AccessError::InvalidInput)
    ));
    assert!(matches!(
        policy
            .override_permission(
                tenant,
                scoped_user,
                Scope::Site(foreign_site),
                allow(Permission::DeviceRead),
                Actor::SYSTEM
            )
            .await,
        Err(AccessError::InvalidInput)
    ));

    for (name, expected) in [
        ("administrator", AccessError::Conflict),
        ("No input at this site", AccessError::Conflict),
        (" Padded", AccessError::InvalidInput),
        ("", AccessError::InvalidInput),
    ] {
        assert_eq!(
            policy
                .create_role(name, &[allow(Permission::DeviceRead)], Actor::SYSTEM)
                .await,
            Err(expected)
        );
    }
    let custom = policy
        .create_role("Read files", &[allow(Permission::FilesRead)], Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .assign_role(tenant, scoped_user, custom, Scope::Tenant, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .authorize(
            &sessions,
            &scoped.token,
            target(workstation),
            Permission::FilesRead,
        )
        .await
        .unwrap();
    check(
        policy
            .authorize(
                &sessions,
                &scoped.token,
                target(workstation),
                Permission::FilesWrite,
            )
            .await,
    );
    check(
        policy
            .authorize(
                &sessions,
                &scoped.token,
                target(workstation),
                Permission::TerminalExecute,
            )
            .await,
    );
    check(
        policy
            .authorize(
                &sessions,
                &scoped.token,
                target(workstation),
                Permission::ScriptExecute,
            )
            .await,
    );
    check(
        policy
            .authorize(
                &sessions,
                &scoped.token,
                target(workstation),
                Permission::DomainJoin,
            )
            .await,
    );
    sqlx::query("UPDATE midas.roles SET enabled=false WHERE id=$1")
        .bind(custom)
        .execute(runtime)
        .await
        .unwrap();
    check(
        policy
            .authorize(
                &sessions,
                &scoped.token,
                target(workstation),
                Permission::FilesRead,
            )
            .await,
    );
    policy
        .set_membership(tenant, user, false, Actor::SYSTEM)
        .await
        .unwrap();
    check(
        policy
            .authorize(
                &sessions,
                &passkey.token,
                target(workstation),
                Permission::DeviceRead,
            )
            .await,
    );
    policy
        .set_membership(tenant, user, true, Actor::SYSTEM)
        .await
        .unwrap();
    policy
        .authorize(
            &sessions,
            &passkey.token,
            target(workstation),
            Permission::DeviceRead,
        )
        .await
        .unwrap();
    sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp()-interval '6 minutes' WHERE id=$1").bind(passkey.info.id).execute(&mut *admin).await.unwrap();
    assert!(matches!(
        policy
            .check_remote_request(&sessions, &passkey.token, target(workstation), uac)
            .await,
        Err(AccessError::FreshPasskeyRequired)
    ));
    assert!(matches!(
        policy
            .check_remote_request(&sessions, &passkey.token, target(server), unattended)
            .await,
        Err(AccessError::FreshPasskeyRequired)
    ));
    policy
        .check_remote_request(&sessions, &passkey.token, target(workstation), normal())
        .await
        .unwrap();
    sessions
        .revoke(&passkey.token, passkey.info.id)
        .await
        .unwrap();
    assert!(matches!(
        policy
            .authorize(
                &sessions,
                &passkey.token,
                target(server),
                Permission::DeviceRead
            )
            .await,
        Err(AccessError::Unauthenticated)
    ));
    policy
        .remove_assignment(tenant, admin_role, Actor::SYSTEM)
        .await
        .unwrap();
    check(
        policy
            .authorize(
                &sessions,
                &fallback.token,
                target(server),
                Permission::TerminalExecute,
            )
            .await,
    );

    for statement in [
        "UPDATE midas.roles SET enabled=false WHERE builtin='administrator'",
        "DELETE FROM midas.roles WHERE builtin='observer'",
        "DELETE FROM midas.role_permissions WHERE role_id='00000000-0000-4000-8000-000000000002'",
        "INSERT INTO midas.permissions VALUES ('wildcard.*')",
        "INSERT INTO midas.roles (id,name,builtin) VALUES ('00000000-0000-4000-8000-000000000004','Forged','administrator')",
    ] {
        let error = sqlx::query(statement).execute(runtime).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().unwrap(), "42501");
    }
    for owner in [tenant, foreign_tenant] {
        let mut tx = runtime.begin().await.unwrap();
        sqlx::query("SELECT set_config('midas.tenant_id',$1,true)")
            .bind(owner.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        for statement in [
            "SELECT tenant_id FROM midas.tenant_memberships",
            "SELECT tenant_id FROM midas.role_assignments",
            "SELECT tenant_id FROM midas.permission_overrides",
        ] {
            let tenants: Vec<Uuid> = sqlx::query_scalar(statement)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
            assert!(tenants.iter().all(|t| *t == owner));
        }
        tx.commit().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenant_memberships")
            .fetch_one(runtime)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
    let mut tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('midas.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let error =
        sqlx::query("INSERT INTO midas.tenant_memberships (tenant_id,operator_id) VALUES ($1,$2)")
            .bind(foreign_tenant)
            .bind(user)
            .execute(&mut *tx)
            .await
            .unwrap_err();
    assert_eq!(error.as_database_error().unwrap().code().unwrap(), "42501");
    tx.rollback().await.unwrap();
    assert!(serde_json::from_str::<Permission>("\"remote.*\"").is_err());
    assert!(serde_json::from_str::<Permission>("\"unknown\"").is_err());
    for permission in Permission::ALL {
        assert_eq!(
            serde_json::from_str::<Permission>(&serde_json::to_string(permission).unwrap())
                .unwrap(),
            *permission
        );
    }
}
