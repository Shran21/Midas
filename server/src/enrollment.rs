use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};

use axum::http::StatusCode;
use midas_protocol::enrollment::{
    ENROLL_PATH, EnrollAnswer, EnrollRequest, Issued, Kind, MAX_BODY_BYTES, MAX_NAME_CHARS,
    Platform, PollAnswer, PollRequest, TOKEN_PREFIX,
};
use rcgen::{
    CertificateParams, CertificateSigningRequestParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
    SanType, SerialNumber,
};
use rustls::pki_types::{CertificateSigningRequestDer, pem::PemObject};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    auth::sessions::{SessionInfo, SessionToken, Sessions},
    authorization::{AccessError, Authorizer, Permission, Target},
    database::Database,
};

const CERTIFICATE_DAYS: i64 = 365;
const REQUEST_DAYS: i64 = 30;
const MAX_TOKEN_HOURS: u32 = 720;

#[derive(Debug)]
pub enum AuthorityError {
    Invalid,
}

impl std::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid agent certificate authority material")
    }
}

impl std::error::Error for AuthorityError {}

/// Signs agent client certificates; only the public key is taken from the request.
pub struct Authority {
    issuer: Issuer<'static, KeyPair>,
}

pub struct Certificate {
    pub der: Vec<u8>,
    pub pem: String,
    pub fingerprint: Vec<u8>,
}

impl Authority {
    pub fn from_pem(certificate: &str, key: &str) -> Result<Self, AuthorityError> {
        let key = KeyPair::from_pem(key).map_err(|_| AuthorityError::Invalid)?;
        let issuer =
            Issuer::from_ca_cert_pem(certificate, key).map_err(|_| AuthorityError::Invalid)?;
        Ok(Self { issuer })
    }

    pub fn issue(
        &self,
        csr: &[u8],
        tenant: Uuid,
        device: Uuid,
    ) -> Result<Certificate, AccessError> {
        let request = verify_csr(csr)?;
        let mut params = CertificateParams::default();
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, device.to_string());
        params.distinguished_name = name;
        params.subject_alt_names = vec![SanType::URI(
            format!("urn:midas:device:{tenant}:{device}")
                .try_into()
                .map_err(|_| AccessError::InvalidInput)?,
        )];
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params.use_authority_key_identifier_extension = true;
        let mut serial = [0u8; 16];
        getrandom::fill(&mut serial).map_err(|_| AccessError::Unavailable)?;
        serial[0] &= 0x7f;
        params.serial_number = Some(SerialNumber::from_slice(&serial));
        let now = time::OffsetDateTime::now_utc();
        params.not_before = now - time::Duration::minutes(5);
        params.not_after = now + time::Duration::days(CERTIFICATE_DAYS);
        let signed = CertificateSigningRequestParams {
            params,
            public_key: request.public_key,
        }
        .signed_by(&self.issuer)
        .map_err(|_| AccessError::Unavailable)?;
        Ok(Certificate {
            fingerprint: fingerprint(signed.der()),
            der: signed.der().to_vec(),
            pem: signed.pem(),
        })
    }
}

/// Verifies the request signature (proof of key possession) and accepts only P-256 keys.
fn verify_csr(der: &[u8]) -> Result<CertificateSigningRequestParams, AccessError> {
    let request =
        CertificateSigningRequestParams::from_der(&CertificateSigningRequestDer::from(der))
            .map_err(|_| AccessError::InvalidInput)?;
    if request.public_key.algorithm() != &PKCS_ECDSA_P256_SHA256 {
        return Err(AccessError::InvalidInput);
    }
    Ok(request)
}

pub fn fingerprint(der: &[u8]) -> Vec<u8> {
    Sha256::digest(der).to_vec()
}

fn digest(domain: &[u8], value: &str) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update([0]);
    hash.update(value.as_bytes());
    hash.finalize().to_vec()
}

fn secret() -> Result<String, AccessError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| AccessError::Unavailable)?;
    Ok(hex::encode(bytes))
}

fn well_formed(secret: &str) -> bool {
    secret.len() == 64 && secret.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenSpec {
    pub site_id: Option<Uuid>,
    pub platform: Platform,
    pub kind: Kind,
    /// A preapproved token is a verified deployment identity and issues without review.
    pub preapproved: bool,
    pub lifetime_hours: u32,
}

/// Returned once; only a digest of the token is stored.
#[derive(Debug, Serialize)]
pub struct CreatedToken {
    pub id: Uuid,
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PendingDevice {
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    pub kind: String,
    pub site_id: Option<Uuid>,
    pub requested_at: i64,
}

/// Agent-facing refusal; the reason stays in the audit trail, not in the response.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Rejected,
    Invalid,
    Unavailable,
}

impl From<sqlx::Error> for Refusal {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentIdentity {
    pub tenant: Uuid,
    pub device: Uuid,
}

/// State pushed to a device's live sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signal {
    pub revoked: bool,
    /// Bumped when the device's directory assignment changes.
    pub refresh: u64,
    /// Bumped when a policy value or the device's groups may have changed.
    pub policy: u64,
    /// Bumped when an operator asks for fresh health checks and inventory.
    pub checks: u64,
}

type Live = Arc<Mutex<HashMap<Uuid, watch::Sender<Signal>>>>;

/// A verified operational session; dropping it unregisters the session.
pub struct Admission {
    pub identity: AgentIdentity,
    pub signal: watch::Receiver<Signal>,
    live: Live,
}

impl Drop for Admission {
    fn drop(&mut self) {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if live
            .get(&self.identity.device)
            .is_some_and(|sender| sender.receiver_count() <= 1)
        {
            live.remove(&self.identity.device);
        }
    }
}

type TokenRow = (Uuid, Uuid, Option<Uuid>, String, String, bool, bool, bool);
type RequestRow = (Vec<u8>, Uuid, Uuid, Option<Vec<u8>>, bool);

pub struct Enrollment {
    pub(crate) pool: PgPool,
    pub(crate) authorizer: Authorizer,
    authority: Authority,
    live: Live,
    /// Tenants whose device list changed; the panel refetches what the operator may see.
    changes: broadcast::Sender<Uuid>,
    pub(crate) refreshed: Mutex<HashMap<Uuid, Instant>>,
}

impl Enrollment {
    pub fn new(database: &Database, authority: Authority) -> Self {
        Self {
            pool: database.pool().clone(),
            authorizer: Authorizer::new(database),
            authority,
            live: Arc::default(),
            changes: broadcast::channel(256).0,
            refreshed: Mutex::default(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Uuid> {
        self.changes.subscribe()
    }

    pub(crate) fn changed(&self, tenant: Uuid) {
        let _ = self.changes.send(tenant);
    }

    pub(crate) async fn tenant_tx(
        &self,
        tenant: Uuid,
    ) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    async fn operator(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        change: bool,
    ) -> Result<SessionInfo, AccessError> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let target = Target {
            tenant,
            device: None,
        };
        if change {
            self.authorizer
                .authorize_identity(&identity, target, Permission::DeviceEnroll)
                .await?;
        } else {
            self.authorizer
                .authorize_view(&identity, target, Permission::DeviceEnroll)
                .await?;
        }
        Ok(identity)
    }

    pub async fn create_token(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        spec: &TokenSpec,
    ) -> Result<CreatedToken, AccessError> {
        let identity = self.operator(sessions, token, tenant, true).await?;
        if spec.lifetime_hours == 0 || spec.lifetime_hours > MAX_TOKEN_HOURS {
            return Err(AccessError::InvalidInput);
        }
        let value = format!("{TOKEN_PREFIX}{}", secret()?);
        let id = Uuid::new_v4();
        let mut tx = self.tenant_tx(tenant).await?;
        if let Some(site) = spec.site_id {
            let known: bool =
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM midas.sites WHERE id = $1)")
                    .bind(site)
                    .fetch_one(&mut *tx)
                    .await?;
            if !known {
                return Err(AccessError::InvalidInput);
            }
        }
        let expires_at: i64 = sqlx::query_scalar("INSERT INTO midas.enrollment_tokens (digest, id, tenant_id, site_id, platform, kind, preapproved, created_by, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, clock_timestamp() + make_interval(hours => $9)) RETURNING extract(epoch FROM expires_at)::bigint")
            .bind(digest(b"enrollment-token", &value)).bind(id).bind(tenant).bind(spec.site_id)
            .bind(spec.platform.as_str()).bind(spec.kind.as_str()).bind(spec.preapproved)
            .bind(identity.operator_id).bind(i32::try_from(spec.lifetime_hours).map_err(|_| AccessError::InvalidInput)?)
            .fetch_one(&mut *tx).await?;
        let mut event = Event::new(
            Action::EnrollmentTokenCreate,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("token_id", id.to_string())
        .detail("platform", spec.platform.as_str())
        .detail("kind", spec.kind.as_str())
        .detail("preapproved", spec.preapproved)
        .detail("lifetime_hours", spec.lifetime_hours);
        event.tenant = Some(tenant);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(CreatedToken {
            id,
            token: value,
            expires_at,
        })
    }

    /// Refusals tied to a known token are audited for its tenant. Unknown or malformed
    /// tokens are only logged, so unauthenticated callers cannot grow the audit trail.
    async fn refuse(&self, known: Option<(Uuid, Uuid)>, reason: &str) -> Refusal {
        tracing::warn!(reason, "agent enrollment refused");
        let Some((tenant, token)) = known else {
            return Refusal::Rejected;
        };
        let mut event = Event::new(Action::EnrollmentRequest, Outcome::Denied, Actor::SYSTEM)
            .detail("reason", reason)
            .detail("token_id", token.to_string());
        event.tenant = Some(tenant);
        let written = match self.tenant_tx(tenant).await {
            Ok(mut tx) => match audit::append_tenant(&mut tx, &event).await {
                Ok(()) => tx.commit().await,
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        if written.is_err() {
            tracing::error!("enrollment refusal could not be audited");
            return Refusal::Unavailable;
        }
        Refusal::Rejected
    }

    /// Handles the first contact of a new agent.
    pub async fn request(&self, request: &EnrollRequest) -> Result<EnrollAnswer, Refusal> {
        let name = request.name.trim();
        if name.is_empty()
            || name.chars().count() > MAX_NAME_CHARS
            || name.chars().any(char::is_control)
        {
            return Err(Refusal::Invalid);
        }
        let csr = CertificateSigningRequestDer::from_pem_slice(request.csr.as_bytes())
            .map_err(|_| Refusal::Invalid)?;
        verify_csr(csr.as_ref()).map_err(|_| Refusal::Invalid)?;
        let Some(secret_part) = request.token.strip_prefix(TOKEN_PREFIX) else {
            return Err(self.refuse(None, "malformed_token").await);
        };
        if !well_formed(secret_part) {
            return Err(self.refuse(None, "malformed_token").await);
        }
        let token_digest = digest(b"enrollment-token", &request.token);
        let mut tx = self.pool.begin().await?;
        let row: Option<TokenRow> = sqlx::query_as("SELECT id, tenant_id, site_id, platform, kind, preapproved, used_at IS NOT NULL, expires_at <= clock_timestamp() FROM midas.enrollment_tokens WHERE digest = $1 FOR UPDATE")
            .bind(&token_digest).fetch_optional(&mut *tx).await?;
        let Some((token_id, tenant, site, platform, kind, preapproved, used, expired)) = row else {
            drop(tx);
            return Err(self.refuse(None, "unknown_token").await);
        };
        let reason = if used {
            Some("token_used")
        } else if expired {
            Some("token_expired")
        } else if platform != request.platform.as_str() || kind != request.kind.as_str() {
            Some("wrong_target")
        } else {
            None
        };
        if let Some(reason) = reason {
            drop(tx);
            return Err(self.refuse(Some((tenant, token_id)), reason).await);
        }
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let device = Uuid::new_v4();
        sqlx::query("INSERT INTO midas.devices (tenant_id, id, site_id, name, kind, platform, enrollment, requested_at, decided_at) VALUES ($1, $2, $3, $4, $5, $6, $7, clock_timestamp(), CASE WHEN $8 THEN clock_timestamp() END)")
            .bind(tenant).bind(device).bind(site).bind(name).bind(&kind).bind(&platform)
            .bind(if preapproved { "active" } else { "pending" }).bind(preapproved)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE midas.enrollment_tokens SET used_at = clock_timestamp(), device_id = $2 WHERE digest = $1")
            .bind(&token_digest).bind(device).execute(&mut *tx).await?;
        let answer = if preapproved {
            let certificate = self
                .authority
                .issue(csr.as_ref(), tenant, device)
                .map_err(|_| Refusal::Unavailable)?;
            self.store(&mut tx, tenant, device, &certificate).await?;
            EnrollAnswer::Issued(Issued {
                device_id: device,
                certificate: certificate.pem,
            })
        } else {
            let request_secret = secret().map_err(|_| Refusal::Unavailable)?;
            let request_id = Uuid::new_v4();
            sqlx::query("INSERT INTO midas.enrollment_requests (id, digest, tenant_id, device_id, csr, expires_at) VALUES ($1, $2, $3, $4, $5, clock_timestamp() + make_interval(days => $6))")
                .bind(request_id).bind(digest(b"enrollment-request", &request_secret)).bind(tenant)
                .bind(device).bind(csr.as_ref()).bind(REQUEST_DAYS as i32)
                .execute(&mut *tx).await?;
            EnrollAnswer::Pending {
                request_id,
                request_secret,
            }
        };
        let mut event = Event::new(Action::EnrollmentRequest, Outcome::Success, Actor::SYSTEM)
            .detail("token_id", token_id.to_string())
            .detail("platform", platform)
            .detail("kind", kind)
            .detail("preapproved", preapproved);
        event.tenant = Some(tenant);
        event.device = Some(device);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        self.changed(tenant);
        Ok(answer)
    }

    async fn store(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        tenant: Uuid,
        device: Uuid,
        certificate: &Certificate,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO midas.agent_credentials (fingerprint, tenant_id, device_id, expires_at) VALUES ($1, $2, $3, clock_timestamp() + make_interval(days => $4))")
            .bind(&certificate.fingerprint).bind(tenant).bind(device).bind(CERTIFICATE_DAYS as i32)
            .execute(&mut **tx).await?;
        Ok(())
    }

    /// Lets a pending agent collect its certificate once an operator approved it.
    pub async fn poll(
        &self,
        request_id: Uuid,
        request_secret: &str,
    ) -> Result<PollAnswer, Refusal> {
        if !well_formed(request_secret) {
            return Err(Refusal::Rejected);
        }
        let row: Option<RequestRow> = sqlx::query_as("SELECT digest, tenant_id, device_id, certificate_pem, expires_at <= clock_timestamp() FROM midas.enrollment_requests WHERE id = $1")
            .bind(request_id).fetch_optional(&self.pool).await?;
        let Some((stored, tenant, device, certificate, expired)) = row else {
            return Err(Refusal::Rejected);
        };
        let presented = digest(b"enrollment-request", request_secret);
        if !constant_time_eq(&stored, &presented) {
            return Err(Refusal::Rejected);
        }
        let mut tx = self.tenant_tx(tenant).await?;
        let state: Option<String> =
            sqlx::query_scalar("SELECT enrollment FROM midas.devices WHERE id = $1")
                .bind(device)
                .fetch_optional(&mut *tx)
                .await?;
        tx.commit().await?;
        match (state.as_deref(), certificate) {
            (Some("active"), Some(pem)) => Ok(PollAnswer::Issued(Issued {
                device_id: device,
                certificate: String::from_utf8(pem).map_err(|_| Refusal::Unavailable)?,
            })),
            (Some("pending"), None) if !expired => Ok(PollAnswer::Pending),
            _ => Err(Refusal::Rejected),
        }
    }

    pub async fn pending(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Vec<PendingDevice>, AccessError> {
        self.operator(sessions, token, tenant, false).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let devices = sqlx::query_as("SELECT d.id, d.name, d.platform, d.kind, d.site_id, extract(epoch FROM d.requested_at)::bigint AS requested_at FROM midas.devices d WHERE d.enrollment = 'pending' AND EXISTS (SELECT 1 FROM midas.enrollment_requests r WHERE r.tenant_id = d.tenant_id AND r.device_id = d.id AND r.expires_at > clock_timestamp()) ORDER BY 6, d.id")
            .fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(devices)
    }

    pub async fn approve(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<(), AccessError> {
        let identity = self.operator(sessions, token, tenant, true).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let changed = sqlx::query("UPDATE midas.devices SET enrollment = 'active', decided_by = $2, decided_at = clock_timestamp() WHERE id = $1 AND enrollment = 'pending'")
            .bind(device).bind(identity.operator_id).execute(&mut *tx).await?.rows_affected();
        let csr: Option<Vec<u8>> = sqlx::query_scalar("SELECT csr FROM midas.enrollment_requests WHERE tenant_id = $1 AND device_id = $2 AND certificate_pem IS NULL AND expires_at > clock_timestamp() FOR UPDATE")
            .bind(tenant).bind(device).fetch_optional(&mut *tx).await?;
        let (1, Some(csr)) = (changed, csr) else {
            return Err(AccessError::Denied);
        };
        let certificate = self.authority.issue(&csr, tenant, device)?;
        sqlx::query("UPDATE midas.enrollment_requests SET certificate_pem = $3 WHERE tenant_id = $1 AND device_id = $2")
            .bind(tenant).bind(device).bind(certificate.pem.as_bytes()).execute(&mut *tx).await?;
        self.store(&mut tx, tenant, device, &certificate).await?;
        self.decided(
            &mut tx,
            Action::EnrollmentApprove,
            &identity,
            tenant,
            device,
        )
        .await?;
        tx.commit().await?;
        self.changed(tenant);
        Ok(())
    }

    pub async fn reject(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<(), AccessError> {
        let identity = self.operator(sessions, token, tenant, true).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let changed = sqlx::query("UPDATE midas.devices SET enrollment = 'rejected', decided_by = $2, decided_at = clock_timestamp() WHERE id = $1 AND enrollment = 'pending'")
            .bind(device).bind(identity.operator_id).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(AccessError::Denied);
        }
        self.decided(&mut tx, Action::EnrollmentReject, &identity, tenant, device)
            .await?;
        tx.commit().await?;
        self.changed(tenant);
        Ok(())
    }

    /// Revokes every credential of the device and closes its live sessions.
    pub async fn revoke(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<(), AccessError> {
        let identity = self.operator(sessions, token, tenant, true).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let changed = sqlx::query("UPDATE midas.devices SET enrollment = 'revoked', decided_by = $2, decided_at = clock_timestamp() WHERE id = $1 AND enrollment IN ('pending', 'active')")
            .bind(device).bind(identity.operator_id).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(AccessError::Denied);
        }
        sqlx::query("UPDATE midas.agent_credentials SET revoked_at = clock_timestamp() WHERE tenant_id = $1 AND device_id = $2 AND revoked_at IS NULL")
            .bind(tenant).bind(device).execute(&mut *tx).await?;
        self.decided(&mut tx, Action::DeviceRevoke, &identity, tenant, device)
            .await?;
        tx.commit().await?;
        self.signal(device, |signal| signal.revoked = true);
        self.changed(tenant);
        Ok(())
    }

    async fn decided(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        action: Action,
        identity: &SessionInfo,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<(), sqlx::Error> {
        let mut event = Event::new(action, Outcome::Success, Actor::session(identity));
        event.tenant = Some(tenant);
        event.device = Some(device);
        audit::append_tenant(tx, &event).await
    }

    async fn identify(&self, certificate: &[u8]) -> Result<Option<AgentIdentity>, sqlx::Error> {
        let row: Option<(Uuid, Uuid)> = sqlx::query_as("SELECT tenant_id, device_id FROM midas.agent_credentials WHERE fingerprint = $1 AND revoked_at IS NULL AND expires_at > clock_timestamp()")
            .bind(fingerprint(certificate)).fetch_optional(&self.pool).await?;
        let Some((tenant, device)) = row else {
            return Ok(None);
        };
        let mut tx = self.tenant_tx(tenant).await?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM midas.devices WHERE id = $1 AND enrollment = 'active')",
        )
        .bind(device)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(active.then_some(AgentIdentity { tenant, device }))
    }

    /// Admits an operational session for a registered, unrevoked certificate.
    pub async fn admit(&self, certificate: &[u8]) -> Option<Admission> {
        let identity = self.identify(certificate).await.ok()??;
        let signal = {
            let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
            live.entry(identity.device)
                .or_insert_with(|| watch::channel(Signal::default()).0)
                .subscribe()
        };
        let admission = Admission {
            identity,
            signal,
            live: self.live.clone(),
        };
        // A revocation between the first lookup and registration is caught here.
        (self.identify(certificate).await.ok()?? == identity && !admission.signal.borrow().revoked)
            .then_some(admission)
    }

    /// Changes the live sessions' signal; false when the device has none.
    pub(crate) fn signal(&self, device: Uuid, change: impl FnOnce(&mut Signal)) -> bool {
        match self
            .live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&device)
        {
            Some(sender) => {
                sender.send_modify(change);
                true
            }
            None => false,
        }
    }
}

pub fn router(enrollment: Arc<Enrollment>) -> axum::Router {
    use axum::routing::post;
    axum::Router::new()
        .route(ENROLL_PATH, post(enroll))
        .route(&format!("{ENROLL_PATH}/{{id}}"), post(collect))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(enrollment)
}

async fn enroll(
    axum::extract::State(enrollment): axum::extract::State<Arc<Enrollment>>,
    body: Result<axum::Json<EnrollRequest>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    let Ok(axum::Json(request)) = body else {
        return refused(Refusal::Invalid);
    };
    match enrollment.request(&request).await {
        Ok(answer @ EnrollAnswer::Issued(_)) => answered(StatusCode::CREATED, &answer),
        Ok(answer) => answered(StatusCode::ACCEPTED, &answer),
        Err(refusal) => refused(refusal),
    }
}

async fn collect(
    axum::extract::State(enrollment): axum::extract::State<Arc<Enrollment>>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    body: Result<axum::Json<PollRequest>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    let Ok(axum::Json(request)) = body else {
        return refused(Refusal::Invalid);
    };
    match enrollment.poll(id, &request.secret).await {
        Ok(answer @ PollAnswer::Issued(_)) => answered(StatusCode::OK, &answer),
        Ok(answer) => answered(StatusCode::ACCEPTED, &answer),
        Err(refusal) => refused(refusal),
    }
}

fn answered(status: StatusCode, body: &impl Serialize) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        status,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        axum::Json(body),
    )
        .into_response()
}

fn refused(refusal: Refusal) -> axum::response::Response {
    let (status, error) = match refusal {
        Refusal::Rejected => (StatusCode::FORBIDDEN, "enrollment_rejected"),
        Refusal::Invalid => (StatusCode::BAD_REQUEST, "invalid_request"),
        Refusal::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable"),
    };
    answered(status, &serde_json::json!({ "error": error }))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> (Authority, rcgen::Certificate) {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(DnType::CommonName, "MIDAS agent CA");
        let certificate = params.self_signed(&key).unwrap();
        (
            Authority::from_pem(&certificate.pem(), &key.serialize_pem()).unwrap(),
            certificate,
        )
    }

    #[test]
    fn issued_certificates_follow_the_server_profile() {
        let (authority, _) = authority();
        let agent = KeyPair::generate().unwrap();
        let mut requested = CertificateParams::new(vec!["evil.example".into()]).unwrap();
        requested.is_ca = IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        requested.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let csr = requested.serialize_request(&agent).unwrap();
        let (tenant, device) = (Uuid::new_v4(), Uuid::new_v4());
        let issued = authority.issue(csr.der(), tenant, device).unwrap();
        let (_, parsed) = x509_parser::parse_x509_certificate(&issued.der).unwrap();
        assert!(
            parsed
                .basic_constraints()
                .unwrap()
                .is_none_or(|constraints| !constraints.value.ca)
        );
        let usage = parsed.extended_key_usage().unwrap().unwrap().value;
        assert!(usage.client_auth && !usage.server_auth && !usage.any);
        let names = &parsed
            .subject_alternative_name()
            .unwrap()
            .unwrap()
            .value
            .general_names;
        assert_eq!(
            names,
            &vec![x509_parser::extensions::GeneralName::URI(&format!(
                "urn:midas:device:{tenant}:{device}"
            ))]
        );
        assert_eq!(
            parsed
                .subject()
                .iter_common_name()
                .next()
                .unwrap()
                .as_str()
                .unwrap(),
            device.to_string()
        );
        let year = time::OffsetDateTime::now_utc() + time::Duration::days(364);
        assert!(parsed.validity().not_after.timestamp() > year.unix_timestamp());
        assert_eq!(issued.fingerprint, fingerprint(&issued.der));
    }

    #[test]
    fn requests_without_valid_possession_proof_or_key_type_are_refused() {
        let (authority, _) = authority();
        let agent = KeyPair::generate().unwrap();
        let mut csr = CertificateParams::default()
            .serialize_request(&agent)
            .unwrap()
            .der()
            .to_vec();
        let last = csr.len() - 1;
        csr[last] ^= 0x01;
        assert!(authority.issue(&csr, Uuid::nil(), Uuid::nil()).is_err());
        let ed25519 = KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap();
        let other = CertificateParams::default()
            .serialize_request(&ed25519)
            .unwrap();
        assert!(
            authority
                .issue(other.der(), Uuid::nil(), Uuid::nil())
                .is_err()
        );
        assert!(
            authority
                .issue(b"not a request", Uuid::nil(), Uuid::nil())
                .is_err()
        );
    }

    #[test]
    fn secrets_compare_in_full() {
        assert!(constant_time_eq(b"abcd", b"abcd"));
        assert!(!constant_time_eq(b"abcd", b"abce"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(well_formed(&"a".repeat(64)) && !well_formed(&"g".repeat(64)));
    }
}
