use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::enrollment::{Admission, AgentEvidence};

use super::*;

pub mod grants;
use grants::{GrantDecision, GrantRequest};
use midas_remote_auth::{Claims, Connection, Mode, Rights, SignedGrant, Target as GrantTarget};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceWindow {
    pub starts_at: i64,
    pub ends_at: i64,
}

impl MaintenanceWindow {
    fn valid(self) -> bool {
        self.starts_at >= 0 && self.starts_at < self.ends_at && self.ends_at <= 253_402_300_799
    }

    pub fn contains(self, now: i64) -> bool {
        self.valid() && self.starts_at <= now && now < self.ends_at
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantPolicy {
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePolicySpec {
    pub enabled: bool,
    pub maintenance_window: Option<MaintenanceWindow>,
}

#[derive(Debug, Serialize)]
pub struct DevicePolicy {
    pub enabled: bool,
    pub tenant_enabled: bool,
    pub maintenance_window: Option<MaintenanceWindow>,
    pub within_maintenance_window: bool,
    pub reboot_confirmation_required: bool,
    pub reconnect_verification_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSelection {
    pub connection_id: Uuid,
    pub os_session_id: String,
    pub generation: Uuid,
}

impl SessionSelection {
    fn valid(&self) -> bool {
        !self.connection_id.is_nil()
            && !self.generation.is_nil()
            && !self.os_session_id.trim().is_empty()
            && self.os_session_id.len() <= 128
            && !self.os_session_id.chars().any(char::is_control)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Vacant,
    Occupied,
    Unknown,
}

/// Created only from the authenticated agent channel, never from a browser's presence claim.
pub struct SessionObservation {
    source: AgentEvidence,
    selection: SessionSelection,
    presence: Presence,
    received_at: Instant,
}

impl SessionObservation {
    pub fn from_agent(
        admission: &Admission,
        selection: SessionSelection,
        presence: Presence,
    ) -> Result<Self> {
        Self::from_request(admission, selection, presence, Instant::now())
    }

    pub(crate) fn from_request(
        admission: &Admission,
        selection: SessionSelection,
        presence: Presence,
        started: Instant,
    ) -> Result<Self> {
        if !selection.valid() || selection.connection_id != admission.connection_id() {
            return Err(AccessError::InvalidInput);
        }
        Ok(Self {
            source: admission.evidence(),
            selection,
            presence,
            received_at: started,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnattendedRequest {
    pub mode: RemoteMode,
    pub uac: bool,
    pub reason: String,
    pub session: SessionSelection,
}

impl UnattendedRequest {
    fn validate(&self) -> Result<()> {
        if !self.session.valid()
            || self.reason.trim().is_empty()
            || self.reason.len() > 512
            || self.reason.chars().any(char::is_control)
        {
            return Err(AccessError::InvalidInput);
        }
        Ok(())
    }

    fn permissions(&self) -> Vec<Permission> {
        let mut permissions = vec![
            Permission::DeviceRead,
            Permission::RemoteView,
            Permission::RemoteControl,
            Permission::RemoteUnattended,
        ];
        if matches!(self.mode, RemoteMode::Administrative) {
            permissions.push(Permission::RemoteAdmin);
        }
        if self.uac {
            permissions.push(Permission::RemoteUacInteract);
        }
        permissions
    }
}

/// A policy result, not a signed or executable grant. Execution must revalidate it.
#[derive(Debug)]
pub enum UnattendedDecision {
    LocalConsentRequired,
    Eligible {
        access: AccessDecision,
        session: SessionSelection,
        expires_at: i64,
    },
}

fn observed_presence(
    target: Target,
    request: &UnattendedRequest,
    observation: &SessionObservation,
    now: Instant,
) -> Result<Presence> {
    if !observation.source.is_live() {
        return Err(AccessError::Offline);
    }
    if observation.source.identity.tenant != target.tenant
        || Some(observation.source.identity.device) != target.device
        || request.session != observation.selection
    {
        return Err(AccessError::Denied);
    }
    if !observation_fresh(observation.received_at, now) || observation.presence == Presence::Unknown
    {
        return Err(AccessError::Offline);
    }
    Ok(observation.presence)
}

fn observation_fresh(received_at: Instant, now: Instant) -> bool {
    now.checked_duration_since(received_at)
        .is_some_and(|age| age < Duration::from_secs(30))
}

impl Authorizer {
    pub(crate) async fn prepare_unattended_request(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        request: &UnattendedRequest,
    ) -> Result<SessionInfo> {
        let identity = sessions.validate(token, false).await?;
        let result = self
            .evaluate(&identity, target, &request.permissions(), true)
            .await
            .and_then(|_| request.validate());
        if let Err(error) = result {
            self.record_unattended_refusal(&identity, target, request, &error)
                .await?;
            return Err(error);
        }
        Ok(identity)
    }

    pub(crate) async fn record_unattended_refusal(
        &self,
        identity: &SessionInfo,
        target: Target,
        request: &UnattendedRequest,
        error: &AccessError,
    ) -> Result<()> {
        let label = match error {
            AccessError::FreshPasskeyRequired => "fresh_passkey_required",
            AccessError::InvalidInput => "invalid_request",
            AccessError::Offline | AccessError::Conflict => "session_unavailable",
            _ => "denied",
        };
        let mut event = Event::new(
            Action::RemoteRequest,
            Outcome::Denied,
            Actor::session(identity),
        )
        .detail("unattended", true)
        .detail("mode", json!(request.mode))
        .detail("uac", request.uac)
        .detail("result", label);
        event.tenant = Some(target.tenant);
        event.device = target.device;
        self.record_decision(&event).await
    }

    pub async fn check_unattended_request(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        request: &UnattendedRequest,
        observation: Option<&SessionObservation>,
    ) -> Result<UnattendedDecision> {
        self.decide_unattended(sessions, token, target, request, observation, None)
            .await
            .map(|(decision, _)| decision)
    }

    pub async fn issue_unattended_grant(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        request: &UnattendedRequest,
        observation: Option<&SessionObservation>,
        issuance: GrantRequest<'_>,
    ) -> Result<GrantDecision> {
        let (decision, grant) = self
            .decide_unattended(
                sessions,
                token,
                target,
                request,
                observation,
                Some(issuance),
            )
            .await?;
        match (decision, grant) {
            (UnattendedDecision::LocalConsentRequired, None) => {
                Ok(GrantDecision::LocalConsentRequired)
            }
            (UnattendedDecision::Eligible { .. }, Some(grant)) => Ok(GrantDecision::Issued(grant)),
            _ => Err(AccessError::Unavailable),
        }
    }

    async fn decide_unattended(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        request: &UnattendedRequest,
        observation: Option<&SessionObservation>,
        issuance: Option<GrantRequest<'_>>,
    ) -> Result<(UnattendedDecision, Option<SignedGrant>)> {
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.unattended_tx(target.tenant).await?;
        let mut signed = None;
        let mut grant_id = None;
        let result = async {
            let access = Self::evaluate_in(&mut tx, &identity, target, &request.permissions(), true).await?;
            request.validate()?;
            let observation = observation.ok_or(AccessError::Offline)?;
            let presence = observed_presence(target, request, observation, Instant::now())?;
            let active: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.devices d JOIN midas.agent_credentials c ON c.tenant_id=d.tenant_id AND c.device_id=d.id WHERE d.tenant_id=$1 AND d.id=$2 AND d.enrollment='active' AND c.fingerprint=$3 AND c.revoked_at IS NULL AND c.expires_at > clock_timestamp())")
                .bind(target.tenant).bind(target.device).bind(&observation.source.credential_fingerprint).fetch_one(&mut *tx).await?;
            if !active { return Err(AccessError::Offline); }
            if presence == Presence::Occupied { return Ok(UnattendedDecision::LocalConsentRequired); }
            let policy = device_policy(&mut tx, target).await?;
            let (now, session_deadline): (i64, i64) = sqlx::query_as("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint, floor(extract(epoch FROM least(expires_at,idle_expires_at,authenticated_at+interval '5 minutes')))::bigint FROM midas.browser_sessions WHERE id=$1")
                .bind(identity.id).fetch_one(&mut *tx).await?;
            let window = policy.maintenance_window.filter(|w| w.contains(now)).ok_or(AccessError::Denied)?;
            let remaining = Duration::from_secs(30).checked_sub(observation.received_at.elapsed()).ok_or(AccessError::Offline)?.as_secs() as i64;
            if remaining == 0 { return Err(AccessError::Offline); }
            let expiry = (now + remaining).min(window.ends_at).min(session_deadline);
            if expiry <= now { return Err(AccessError::FreshPasskeyRequired); }
            if let Some(issuance) = &issuance {
                let id = Uuid::new_v4();
                let claims = Claims {
                    id, challenge: issuance.challenge, operator: identity.operator_id,
                    target: GrantTarget { connection: Connection { tenant: target.tenant, device: target.device.ok_or(AccessError::InvalidInput)?, id: request.session.connection_id },
                        session: request.session.os_session_id.parse().map_err(|_| AccessError::InvalidInput)?, generation: request.session.generation },
                    rights: Rights { mode: match request.mode { RemoteMode::Normal => Mode::Normal, RemoteMode::Administrative => Mode::Administrative }, uac: request.uac },
                    issued_at: now, expires_at: expiry,
                };
                signed = Some(issuance.authority.sign(&claims, &observation.source)?);
                grant_id = Some(id);
            }
            Ok(UnattendedDecision::Eligible { access, session: request.session.clone(), expires_at: expiry })
        }.await;
        let (outcome, label) = match &result {
            Ok(UnattendedDecision::Eligible { .. }) => (Outcome::Success, "eligible_for_grant"),
            Ok(UnattendedDecision::LocalConsentRequired) => {
                (Outcome::Denied, "local_consent_required")
            }
            Err(AccessError::FreshPasskeyRequired) => (Outcome::Denied, "fresh_passkey_required"),
            Err(AccessError::InvalidInput) => (Outcome::Denied, "invalid_request"),
            Err(AccessError::Offline) => (Outcome::Denied, "session_unavailable"),
            Err(AccessError::Denied | AccessError::Unauthenticated) => (Outcome::Denied, "denied"),
            Err(_) => return result.map(|decision| (decision, None)),
        };
        let mut event = Event::new(Action::RemoteRequest, outcome, Actor::session(&identity))
            .detail("unattended", true)
            .detail("mode", json!(request.mode))
            .detail("uac", request.uac)
            .detail("result", label);
        event.tenant = Some(target.tenant);
        event.device = target.device;
        if let Some(id) = grant_id {
            event.action = Action::RemoteGrantIssued;
            event = event
                .detail("grant_id", id.to_string())
                .detail("result", "issued");
        }
        if result.is_ok() {
            observed_presence(
                target,
                request,
                observation.ok_or(AccessError::Offline)?,
                Instant::now(),
            )?;
            event = event
                .detail("reason", request.reason.trim())
                .detail("session", json!(request.session));
            if let Ok(UnattendedDecision::Eligible { expires_at, .. }) = &result {
                event = event.detail("expires_at", *expires_at);
            }
            audit::append_tenant(&mut tx, &event).await?;
            tx.commit().await?;
            observed_presence(
                target,
                request,
                observation.ok_or(AccessError::Offline)?,
                Instant::now(),
            )?;
        } else {
            drop(tx);
            self.record_decision(&event).await?;
        }
        result.map(|decision| (decision, signed))
    }

    async fn unattended_tx(&self, tenant: Uuid) -> Result<Transaction<'_, Postgres>> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    pub async fn tenant_unattended_policy(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<TenantPolicy> {
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.tenant_transaction(tenant, true).await?;
        Self::evaluate_in(
            &mut tx,
            &identity,
            Target {
                tenant,
                device: None,
            },
            &[Permission::AccessManage],
            false,
        )
        .await?;
        let enabled =
            sqlx::query_scalar("SELECT unattended_enabled FROM midas.tenants WHERE id=$1")
                .bind(tenant)
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(TenantPolicy { enabled })
    }

    pub async fn set_tenant_unattended_policy(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        policy: &TenantPolicy,
    ) -> Result<()> {
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.unattended_tx(tenant).await?;
        let target = Target {
            tenant,
            device: None,
        };
        Self::evaluate_in(
            &mut tx,
            &identity,
            target,
            &[Permission::AccessManage],
            true,
        )
        .await?;
        if policy.enabled {
            can_enable(&mut tx, &identity, target).await?;
        }
        let previous: bool = sqlx::query_scalar(
            "SELECT unattended_enabled FROM midas.tenants WHERE id=$1 FOR UPDATE",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("UPDATE midas.tenants SET unattended_enabled=$2 WHERE id=$1")
            .bind(tenant)
            .bind(policy.enabled)
            .execute(&mut *tx)
            .await?;
        let mut event = Event::new(
            Action::UnattendedPolicySet,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("scope", "tenant")
        .detail("previous_enabled", previous)
        .detail("enabled", policy.enabled);
        event.tenant = Some(tenant);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn device_unattended_policy(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
    ) -> Result<DevicePolicy> {
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.tenant_transaction(target.tenant, true).await?;
        Self::evaluate_in(&mut tx, &identity, target, &[Permission::DeviceRead], false).await?;
        let policy = device_policy(&mut tx, target).await?;
        tx.commit().await?;
        Ok(policy)
    }

    pub async fn set_device_unattended_policy(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        target: Target,
        policy: &DevicePolicySpec,
    ) -> Result<()> {
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.unattended_tx(target.tenant).await?;
        Self::evaluate_in(
            &mut tx,
            &identity,
            target,
            &[Permission::AccessManage],
            true,
        )
        .await?;
        let previous = device_policy(&mut tx, target).await?;
        let now: i64 =
            sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
                .fetch_one(&mut *tx)
                .await?;
        if policy.maintenance_window.is_some_and(|w| !w.valid())
            || (policy.enabled && !policy.maintenance_window.is_some_and(|w| w.ends_at > now))
        {
            return Err(AccessError::InvalidInput);
        }
        if policy.enabled {
            can_enable(&mut tx, &identity, target).await?;
        }
        let window = policy.maintenance_window;
        sqlx::query("UPDATE midas.devices SET unattended_enabled=$3, unattended_window_start=$4, unattended_window_end=$5 WHERE tenant_id=$1 AND id=$2")
            .bind(target.tenant).bind(target.device).bind(policy.enabled)
            .bind(window.map(|w| w.starts_at)).bind(window.map(|w| w.ends_at))
            .execute(&mut *tx).await?;
        let mut event = Event::new(
            Action::UnattendedPolicySet,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("scope", "device")
        .detail("previous_enabled", previous.enabled)
        .detail("previous_window", json!(previous.maintenance_window))
        .detail("enabled", policy.enabled)
        .detail("maintenance_window", json!(window));
        event.tenant = Some(target.tenant);
        event.device = target.device;
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn can_enable(
    tx: &mut Transaction<'_, Postgres>,
    identity: &SessionInfo,
    target: Target,
) -> Result<()> {
    // Check held rights without the opt-in being edited; otherwise a disabled policy could never be enabled.
    let name = Permission::RemoteUnattended.as_str();
    let held = if let Some(device) = target.device {
        device_grants(
            tx,
            target.tenant,
            identity.operator_id,
            &[name],
            Some(device),
        )
        .await?
        .iter()
        .any(|(_, p)| p == name)
    } else {
        allowed(&tenant_rules(tx, target.tenant, identity.operator_id, &[name]).await?)
            .contains(&Permission::RemoteUnattended)
    };
    if !held {
        return Err(AccessError::Escalation);
    }
    Ok(())
}

type DevicePolicyRow = (bool, bool, Option<i64>, Option<i64>, i64);

async fn device_policy(tx: &mut Transaction<'_, Postgres>, target: Target) -> Result<DevicePolicy> {
    let device = target.device.ok_or(AccessError::InvalidInput)?;
    let row: Option<DevicePolicyRow> = sqlx::query_as("SELECT d.unattended_enabled,t.unattended_enabled,d.unattended_window_start,d.unattended_window_end,floor(extract(epoch FROM clock_timestamp()))::bigint FROM midas.devices d JOIN midas.tenants t ON t.id=d.tenant_id WHERE d.tenant_id=$1 AND d.id=$2 AND d.kind='server'")
        .bind(target.tenant).bind(device).fetch_optional(&mut **tx).await?;
    let (enabled, tenant_enabled, start, end, now) = row.ok_or(AccessError::Denied)?;
    let maintenance_window = start
        .zip(end)
        .map(|(starts_at, ends_at)| MaintenanceWindow { starts_at, ends_at });
    Ok(DevicePolicy {
        enabled,
        tenant_enabled,
        maintenance_window,
        within_maintenance_window: maintenance_window.is_some_and(|w| w.contains(now)),
        reboot_confirmation_required: true,
        reconnect_verification_required: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_window_is_start_inclusive_and_end_exclusive() {
        let window = MaintenanceWindow {
            starts_at: 100,
            ends_at: 200,
        };
        assert!(!window.contains(99));
        assert!(window.contains(100));
        assert!(window.contains(199));
        assert!(!window.contains(200));
        for invalid in [
            MaintenanceWindow {
                starts_at: -1,
                ends_at: 200,
            },
            MaintenanceWindow {
                starts_at: 200,
                ends_at: 100,
            },
            MaintenanceWindow {
                starts_at: 0,
                ends_at: i64::MAX,
            },
        ] {
            assert!(!invalid.contains(100));
        }
    }

    #[test]
    fn future_and_expired_observations_cannot_authorize_a_session() {
        let now = Instant::now();
        assert!(observation_fresh(now, now));
        assert!(observation_fresh(now, now + Duration::from_secs(29)));
        assert!(!observation_fresh(now, now + Duration::from_secs(30)));
        assert!(!observation_fresh(now + Duration::from_secs(1), now));
    }
}
