//! Remote support for a signed-in Windows user, started from the admin panel. The server signs a
//! grant only after the agent reports that the local user allowed this exact request.

use std::time::{Duration, Instant};

use midas_remote_auth::{Claims, Connection, Mode, Rights, SignedGrant, Target as GrantTarget};
use serde::Deserialize;

use super::{
    unattended::{
        Presence, SessionObservation, SessionSelection,
        grants::{GrantAuthority, GrantRequest},
        observed_presence,
    },
    *,
};

/// How long the signed-in user has to answer; the agent closes its window at the same limit.
pub const CONSENT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendedRequest {
    pub mode: RemoteMode,
    pub uac: bool,
    pub session: SessionSelection,
    #[serde(default)]
    pub transfer: Transfer,
}

/// The session and rights an attended request names, as its grants bind them.
fn granted(target: Target, request: &AttendedRequest) -> Result<(GrantTarget, Rights)> {
    Ok((
        GrantTarget {
            connection: Connection {
                tenant: target.tenant,
                device: target.device.ok_or(AccessError::InvalidInput)?,
                id: request.session.connection_id,
            },
            session: request
                .session
                .os_session_id
                .parse()
                .map_err(|_| AccessError::InvalidInput)?,
            generation: request.session.generation,
        },
        Rights {
            mode: match request.mode {
                RemoteMode::Normal => Mode::Normal,
                RemoteMode::Administrative => Mode::Administrative,
            },
            uac: request.uac,
            transfer: request.transfer.rights(),
        },
    ))
}

/// Every permission an attended session of this mode needs; administrative mode, UAC and each
/// kind of transfer add their own, separately granted permission.
fn mode_permissions(mode: RemoteMode, uac: bool, transfer: Transfer) -> Vec<Permission> {
    let mut permissions = vec![
        Permission::DeviceRead,
        Permission::RemoteView,
        Permission::RemoteControl,
    ];
    if matches!(mode, RemoteMode::Administrative) {
        permissions.push(Permission::RemoteAdmin);
    }
    if uac {
        permissions.push(Permission::RemoteUacInteract);
    }
    permissions.extend(transfer.permissions());
    permissions
}

impl AttendedRequest {
    fn permissions(&self) -> Vec<Permission> {
        mode_permissions(self.mode, self.uac, self.transfer)
    }

    fn fresh_passkey(&self) -> bool {
        self.permissions()
            .iter()
            .any(|permission| permission.requires_fresh_passkey())
    }
}

/// One attended request, as the operator's browser session made it.
pub struct Asked<'a> {
    pub sessions: &'a Sessions,
    pub token: &'a SessionToken,
    pub target: Target,
    pub request: &'a AttendedRequest,
    pub id: Uuid,
}

/// The verified requester and customer that the local consent window names.
pub struct Requester {
    pub identity: SessionInfo,
    pub operator_name: String,
    pub customer_name: String,
}

pub(crate) fn refusal(error: &AccessError) -> &'static str {
    match error {
        AccessError::FreshPasskeyRequired => "fresh_passkey_required",
        AccessError::InvalidInput => "invalid_request",
        AccessError::Offline | AccessError::Conflict => "session_unavailable",
        AccessError::Unauthenticated => "unauthenticated",
        AccessError::Unavailable => "unavailable",
        _ => "denied",
    }
}

impl Authorizer {
    /// A later operation of an active attended session, such as starting its picture. The
    /// operator must hold every permission of the session's mode now; passkey freshness belongs
    /// to the session start and is not asked again.
    pub(crate) async fn authorize_attended_operation(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        mode: RemoteMode,
        uac: bool,
        transfer: Transfer,
    ) -> Result<AccessDecision> {
        let identity = sessions.validate(token, false).await?;
        self.evaluate(
            &identity,
            target,
            &mode_permissions(mode, uac, transfer),
            false,
        )
        .await
    }

    /// Renews an active attended session. The operator's browser session must still be valid and
    /// hold every permission of the mode, and the agent credential must be current; the passkey
    /// is not asked again. The renewal names the last applied grant, so only that lease extends.
    pub(crate) async fn renew_attended_grant(
        &self,
        asked: &Asked<'_>,
        authority: &GrantAuthority,
        observation: &SessionObservation,
        previous: Uuid,
    ) -> Result<SignedGrant> {
        let Asked {
            sessions,
            token,
            target,
            request,
            ..
        } = *asked;
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.unattended_tx(target.tenant).await?;
        Self::evaluate_in(&mut tx, &identity, target, &request.permissions(), false).await?;
        let active: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.devices d JOIN midas.agent_credentials c ON c.tenant_id=d.tenant_id AND c.device_id=d.id WHERE d.tenant_id=$1 AND d.id=$2 AND d.enrollment='active' AND c.fingerprint=$3 AND c.revoked_at IS NULL AND c.expires_at > clock_timestamp())")
            .bind(target.tenant).bind(target.device).bind(&observation.source.credential_fingerprint).fetch_one(&mut *tx).await?;
        if !active {
            return Err(AccessError::Offline);
        }
        let (now, session_deadline): (i64, i64) = sqlx::query_as("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint, floor(extract(epoch FROM least(expires_at, idle_expires_at)))::bigint FROM midas.browser_sessions WHERE id=$1")
            .bind(identity.id).fetch_one(&mut *tx).await?;
        drop(tx);
        let expiry = (now + midas_remote_auth::MAX_LIFETIME_SECONDS as i64).min(session_deadline);
        if expiry <= now {
            return Err(AccessError::Unauthenticated);
        }
        let (grant_target, rights) = granted(target, request)?;
        let claims = Claims {
            id: Uuid::new_v4(),
            challenge: previous,
            operator: identity.operator_id,
            target: grant_target,
            rights,
            issued_at: now,
            expires_at: expiry,
        };
        authority.sign(&claims, &observation.source)
    }

    /// Checks the request and records a refusal; on success it names who asks, for the window.
    pub(crate) async fn prepare_attended_request(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        request: &AttendedRequest,
        id: Uuid,
    ) -> Result<Requester> {
        let identity = sessions.validate(token, false).await?;
        let result = async {
            if !request.session.valid() {
                return Err(AccessError::InvalidInput);
            }
            let mut tx = self.tenant_transaction(target.tenant, true).await?;
            Self::evaluate_in(&mut tx, &identity, target, &request.permissions(), true).await?;
            let names: (String, String) = sqlx::query_as("SELECT o.display_name, t.name FROM midas.operators o JOIN midas.tenants t ON t.id = $2 WHERE o.id = $1")
                .bind(identity.operator_id).bind(target.tenant).fetch_one(&mut *tx).await?;
            tx.commit().await?;
            Ok(names)
        }
        .await;
        match result {
            Ok((operator_name, customer_name)) => Ok(Requester {
                identity,
                operator_name,
                customer_name,
            }),
            Err(error) => {
                self.record_attended(
                    &identity,
                    target,
                    request,
                    id,
                    (Action::RemoteRequest, Outcome::Denied, refusal(&error)),
                )
                .await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn record_attended(
        &self,
        identity: &SessionInfo,
        target: Target,
        request: &AttendedRequest,
        id: Uuid,
        (action, outcome, label): (Action, Outcome, &str),
    ) -> Result<()> {
        let mut event = Event::new(action, outcome, Actor::session(identity))
            .detail("unattended", false)
            .detail("request_id", id.to_string())
            .detail("mode", json!(request.mode))
            .detail("uac", request.uac)
            .detail("transfer", json!(request.transfer))
            .detail("session", json!(request.session))
            .detail("result", label);
        event.tenant = Some(target.tenant);
        event.device = target.device;
        self.record_decision(&event).await
    }

    /// Signs a grant for the occupied desktop whose user has just allowed request `id`. The
    /// permissions, the device credential and passkey freshness are checked again, because the
    /// local answer can take a minute.
    pub async fn issue_attended_grant(
        &self,
        asked: &Asked<'_>,
        observation: &SessionObservation,
        issuance: GrantRequest<'_>,
    ) -> Result<SignedGrant> {
        let Asked {
            sessions,
            token,
            target,
            request,
            id,
        } = *asked;
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.unattended_tx(target.tenant).await?;
        let result = async {
            Self::evaluate_in(&mut tx, &identity, target, &request.permissions(), true).await?;
            if !request.session.valid() {
                return Err(AccessError::InvalidInput);
            }
            if observed_presence(target, &request.session, observation, Instant::now())?
                != Presence::Occupied
            {
                return Err(AccessError::Offline);
            }
            let active: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.devices d JOIN midas.agent_credentials c ON c.tenant_id=d.tenant_id AND c.device_id=d.id WHERE d.tenant_id=$1 AND d.id=$2 AND d.enrollment='active' AND c.fingerprint=$3 AND c.revoked_at IS NULL AND c.expires_at > clock_timestamp())")
                .bind(target.tenant).bind(target.device).bind(&observation.source.credential_fingerprint).fetch_one(&mut *tx).await?;
            if !active {
                return Err(AccessError::Offline);
            }
            let fresh = request.fresh_passkey();
            let (now, session_deadline): (i64, i64) = sqlx::query_as("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint, floor(extract(epoch FROM least(expires_at, idle_expires_at, CASE WHEN $2 THEN authenticated_at + interval '5 minutes' ELSE expires_at END)))::bigint FROM midas.browser_sessions WHERE id=$1")
                .bind(identity.id).bind(fresh).fetch_one(&mut *tx).await?;
            let remaining = Duration::from_secs(30)
                .checked_sub(observation.received_at.elapsed())
                .ok_or(AccessError::Offline)?
                .as_secs() as i64;
            if remaining == 0 {
                return Err(AccessError::Offline);
            }
            let expiry = (now + remaining).min(session_deadline);
            if expiry <= now {
                return Err(if fresh {
                    AccessError::FreshPasskeyRequired
                } else {
                    AccessError::Unauthenticated
                });
            }
            let (grant_target, rights) = granted(target, request)?;
            let claims = Claims {
                id: Uuid::new_v4(),
                challenge: issuance.challenge,
                operator: identity.operator_id,
                target: grant_target,
                rights,
                issued_at: now,
                expires_at: expiry,
            };
            let signed = issuance.authority.sign(&claims, &observation.source)?;
            Ok((signed, claims.id, expiry))
        }
        .await;
        let mut event = Event::new(
            Action::RemoteGrantIssued,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("unattended", false)
        .detail("request_id", id.to_string())
        .detail("mode", json!(request.mode))
        .detail("uac", request.uac)
        .detail("transfer", json!(request.transfer))
        .detail("session", json!(request.session));
        event.tenant = Some(target.tenant);
        event.device = target.device;
        match result {
            Ok((signed, grant, expiry)) => {
                event = event
                    .detail("grant_id", grant.to_string())
                    .detail("expires_at", expiry)
                    .detail("result", "issued");
                audit::append_tenant(&mut tx, &event).await?;
                tx.commit().await?;
                observed_presence(target, &request.session, observation, Instant::now())?;
                Ok(signed)
            }
            Err(error) => {
                drop(tx);
                event.action = Action::RemoteRequest;
                event.outcome = Outcome::Denied;
                event = event.detail("result", refusal(&error));
                self.record_decision(&event).await?;
                Err(error)
            }
        }
    }
}
