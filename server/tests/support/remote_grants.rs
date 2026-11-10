use midas_agent_core::remote::{
    Channel, DesktopKind, Observation, Presence as NativePresence, Worker,
};
use midas_remote_auth::{
    Connection, Mode, Rights, Target as GrantTarget, verify as verify_signature,
};
use midas_server::{
    audit::Actor,
    auth::sessions::{SessionToken, Sessions},
    authorization::{
        AccessError, Authorizer, Effect, Permission, RemoteMode, Rule, Scope, Target,
        unattended::{
            Presence, SessionObservation, SessionSelection, UnattendedRequest,
            grants::{GrantAuthority, GrantDecision, GrantRequest},
        },
    },
    database::driver as sqlx,
    enrollment::{Admission, Certificate, Enrollment},
};
use sqlx::PgConnection;
use uuid::Uuid;

pub struct Context<'a> {
    pub policy: &'a Authorizer,
    pub sessions: &'a Sessions,
    pub token: &'a SessionToken,
    pub target: Target,
    pub admission: &'a Admission,
    pub agents: &'a Enrollment,
    pub certificate: &'a Certificate,
    pub operator: Uuid,
}

pub async fn verify(ctx: Context<'_>, admin: &mut PgConnection) {
    let authority = GrantAuthority::for_agent(ctx.admission).unwrap();
    let connection = Connection {
        tenant: ctx.target.tenant,
        device: ctx.target.device.unwrap(),
        id: ctx.admission.connection_id(),
    };
    let target = GrantTarget {
        connection,
        session: 7,
        generation: Uuid::new_v4(),
    };
    let channel = Channel::from_authenticated_server(connection, authority.public_key()).unwrap();
    let pending = channel
        .prepare(
            target,
            ctx.operator,
            Rights {
                mode: Mode::Administrative,
                uac: true,
            },
        )
        .unwrap();
    let challenge = pending.challenge();
    let request = UnattendedRequest {
        mode: RemoteMode::Administrative,
        uac: true,
        reason: "Grant integration trial".into(),
        session: SessionSelection {
            connection_id: connection.id,
            os_session_id: "7".into(),
            generation: target.generation,
        },
    };
    let vacant =
        SessionObservation::from_agent(ctx.admission, request.session.clone(), Presence::Vacant)
            .unwrap();
    let occupied =
        SessionObservation::from_agent(ctx.admission, request.session.clone(), Presence::Occupied)
            .unwrap();
    let unknown =
        SessionObservation::from_agent(ctx.admission, request.session.clone(), Presence::Unknown)
            .unwrap();
    let issue = || GrantRequest {
        authority: &authority,
        challenge,
    };
    let grant = ctx
        .policy
        .issue_unattended_grant(
            ctx.sessions,
            ctx.token,
            ctx.target,
            &request,
            Some(&vacant),
            issue(),
        )
        .await
        .unwrap();
    let GrantDecision::Issued(grant) = grant else {
        panic!("vacant server should receive a grant");
    };
    let checked = verify_signature(&authority.public_key(), &grant).unwrap();
    assert_eq!(checked.claims().target, target);
    assert_eq!(checked.claims().operator, ctx.operator);
    let local = Observation {
        target,
        presence: NativePresence::Vacant,
        desktop: DesktopKind::User,
        desktop_epoch: Uuid::new_v4(),
    };
    let lease = pending.accept(&grant, local).unwrap();
    let permit = lease.prepare_input(local, Worker::User).unwrap();
    assert_eq!(lease.commit_input(permit, local), Ok(()));
    let details:String=sqlx::query_scalar("SELECT details::text FROM midas.tenant_audit WHERE tenant_id=$1 AND action='remote.grant_issued' ORDER BY sequence DESC LIMIT 1").bind(ctx.target.tenant).fetch_one(&mut *admin).await.unwrap();
    let details: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(details["grant_id"], checked.claims().id.to_string());
    assert_eq!(details["reason"], request.reason);
    assert_eq!(details["expires_at"], checked.claims().expires_at);
    assert!(details.get("signature").is_none() && details.get("payload").is_none());
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&occupied),
                issue()
            )
            .await
            .unwrap(),
        GrantDecision::LocalConsentRequired
    ));
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&unknown),
                issue()
            )
            .await,
        Err(AccessError::Offline)
    ));
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&vacant),
                GrantRequest {
                    authority: &authority,
                    challenge: Uuid::nil()
                }
            )
            .await,
        Err(AccessError::InvalidInput)
    ));
    let other = ctx.agents.admit(&ctx.certificate.der).await.unwrap();
    let other_authority = GrantAuthority::for_agent(&other).unwrap();
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&vacant),
                GrantRequest {
                    authority: &other_authority,
                    challenge
                }
            )
            .await,
        Err(AccessError::Denied)
    ));
    drop(other);
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&vacant),
                GrantRequest {
                    authority: &other_authority,
                    challenge
                }
            )
            .await,
        Err(AccessError::Offline)
    ));
    for permission in [
        Permission::RemoteControl,
        Permission::RemoteAdmin,
        Permission::RemoteUacInteract,
        Permission::RemoteUnattended,
    ] {
        let override_id = ctx
            .policy
            .override_permission(
                connection.tenant,
                ctx.operator,
                Scope::Device(connection.device),
                Rule {
                    permission,
                    effect: Effect::Deny,
                },
                Actor::SYSTEM,
            )
            .await
            .unwrap();
        assert!(matches!(
            ctx.policy
                .issue_unattended_grant(
                    ctx.sessions,
                    ctx.token,
                    ctx.target,
                    &request,
                    Some(&vacant),
                    issue()
                )
                .await,
            Err(AccessError::Denied)
        ));
        ctx.policy
            .remove_override(connection.tenant, override_id, Actor::SYSTEM)
            .await
            .unwrap();
    }

    sqlx::query(
        "UPDATE midas.agent_credentials SET revoked_at=clock_timestamp() WHERE fingerprint=$1",
    )
    .bind(&ctx.certificate.fingerprint)
    .execute(&mut *admin)
    .await
    .unwrap();
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&vacant),
                issue()
            )
            .await,
        Err(AccessError::Offline)
    ));
    sqlx::query("UPDATE midas.agent_credentials SET revoked_at=NULL WHERE fingerprint=$1")
        .bind(&ctx.certificate.fingerprint)
        .execute(&mut *admin)
        .await
        .unwrap();
    sqlx::query("UPDATE midas.tenants SET unattended_enabled=false WHERE id=$1")
        .bind(ctx.target.tenant)
        .execute(&mut *admin)
        .await
        .unwrap();
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&vacant),
                issue()
            )
            .await,
        Err(AccessError::Denied)
    ));
    sqlx::query("UPDATE midas.tenants SET unattended_enabled=true WHERE id=$1")
        .bind(ctx.target.tenant)
        .execute(&mut *admin)
        .await
        .unwrap();
    sqlx::query("UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp()-interval '6 minutes' WHERE operator_id=$1").bind(ctx.operator).execute(&mut *admin).await.unwrap();
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&vacant),
                issue()
            )
            .await,
        Err(AccessError::FreshPasskeyRequired)
    ));
    sqlx::query(
        "UPDATE midas.browser_sessions SET authenticated_at=clock_timestamp() WHERE operator_id=$1",
    )
    .bind(ctx.operator)
    .execute(&mut *admin)
    .await
    .unwrap();

    sqlx::raw_sql("CREATE FUNCTION midas.reject_grant_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.action='remote.grant_issued' THEN RAISE EXCEPTION 'test'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_grant_audit BEFORE INSERT ON midas.tenant_audit FOR EACH ROW EXECUTE FUNCTION midas.reject_grant_audit();").execute(&mut *admin).await.unwrap();
    assert!(matches!(
        ctx.policy
            .issue_unattended_grant(
                ctx.sessions,
                ctx.token,
                ctx.target,
                &request,
                Some(&vacant),
                issue()
            )
            .await,
        Err(AccessError::Unavailable)
    ));
    sqlx::raw_sql("DROP TRIGGER reject_grant_audit ON midas.tenant_audit; DROP FUNCTION midas.reject_grant_audit();").execute(&mut *admin).await.unwrap();
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM midas.tenant_audit WHERE tenant_id=$1 AND action='remote.grant_issued'").bind(ctx.target.tenant).fetch_one(&mut *admin).await.unwrap();
    assert_eq!(count, 1);
    channel.revoke();
    assert!(lease.capture(local, Worker::User).is_err());
}
