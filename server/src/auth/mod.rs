mod challenges;
mod crypto;
pub mod directory;
pub mod http;
pub mod sessions;
pub mod setup;

use std::{
    fmt,
    net::IpAddr,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sqlx::PgPool;
use totp_rs::{Algorithm, Builder, Totp};
use uuid::Uuid;
use webauthn_rs::prelude::*;
use zeroize::Zeroizing;

use crate::database::Database;
use challenges::{Ceremony, Challenges, Pending};
use crypto::{Passwords, Vault, digest, random};

type Result<T> = std::result::Result<T, AuthError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    InvalidInput,
    InvalidCredentials,
    RateLimited,
    Busy,
    Unavailable,
    Conflict,
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid authentication input",
            Self::InvalidCredentials => "authentication failed",
            Self::RateLimited => "authentication attempt limit reached",
            Self::Busy => "authentication capacity temporarily unavailable",
            Self::Unavailable => "authentication service unavailable",
            Self::Conflict => "credential already registered or changed",
        })
    }
}
impl std::error::Error for AuthError {}
impl From<sqlx::Error> for AuthError {
    fn from(error: sqlx::Error) -> Self {
        if error
            .as_database_error()
            .is_some_and(|e| e.is_unique_violation())
        {
            Self::Conflict
        } else {
            Self::Unavailable
        }
    }
}

pub struct BrowserBinding(Zeroizing<[u8; 32]>);
impl BrowserBinding {
    pub fn random() -> Result<Self> {
        Ok(Self(Zeroizing::new(random()?)))
    }
    pub fn from_bytes(bytes: Zeroizing<[u8; 32]>) -> Result<Self> {
        if *bytes == [0; 32] {
            return Err(AuthError::InvalidInput);
        }
        Ok(Self(bytes))
    }
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    fn digest(&self) -> Vec<u8> {
        digest(b"browser-binding", self.0.as_slice())
    }
}

pub struct AuthConfig {
    pub origin: Url,
    pub encryption_key: Zeroizing<[u8; 32]>,
    pub password_workers: usize,
    pub challenge_lifetime: Duration,
    pub challenge_capacity: usize,
    pub account_attempts: i32,
    pub source_attempts: i32,
    pub attempt_window: Duration,
}

impl AuthConfig {
    pub fn new(origin: Url, encryption_key: Zeroizing<[u8; 32]>) -> Self {
        Self {
            origin,
            encryption_key,
            password_workers: 2,
            challenge_lifetime: Duration::from_secs(300),
            challenge_capacity: 1024,
            account_attempts: 20,
            source_attempts: 100,
            attempt_window: Duration::from_secs(300),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationMethod {
    Passkey,
    PasswordTotp,
    RecoveryCode,
    Directory,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Profile {
    pub login: String,
    pub display_name: String,
    pub platform_admin: bool,
}

#[derive(Debug)]
pub struct Authenticated {
    operator: Uuid,
    version: i64,
    method: AuthenticationMethod,
    verified_at: SystemTime,
}

impl Authenticated {
    pub fn operator_id(&self) -> Uuid {
        self.operator
    }
    pub fn credential_version(&self) -> i64 {
        self.version
    }
    pub fn method(&self) -> AuthenticationMethod {
        self.method
    }
    pub fn verified_at(&self) -> SystemTime {
        self.verified_at
    }
}

#[derive(Serialize)]
pub struct Issued<T> {
    pub id: String,
    pub options: T,
}

pub struct TotpEnrollment {
    operator: Uuid,
    version: i64,
    secret: Zeroizing<Vec<u8>>,
    expires: Instant,
    login: String,
}

impl TotpEnrollment {
    pub fn provisioning_uri(&self) -> Result<Zeroizing<String>> {
        let totp = totp(self.secret.to_vec(), &self.login)?;
        totp.to_url()
            .map(Zeroizing::new)
            .map_err(|_| AuthError::InvalidInput)
    }
}

#[derive(sqlx::FromRow)]
struct Account {
    id: Uuid,
    login: String,
    display_name: String,
    credential_version: i64,
    password_hash: Option<String>,
    totp_secret: Option<Vec<u8>>,
}

pub struct AuthService {
    pool: PgPool,
    webauthn: Webauthn,
    passwords: Passwords,
    dummy_hash: String,
    vault: Vault,
    challenges: Challenges,
    account_attempts: i32,
    source_attempts: i32,
    attempt_window: i64,
    last_cleanup: AtomicU64,
}

impl AuthService {
    pub async fn new(database: &Database, config: AuthConfig) -> Result<Self> {
        if config.origin.scheme() != "https"
            || config.origin.domain().is_none()
            || config.origin.path() != "/"
            || config.origin.query().is_some()
            || config.origin.fragment().is_some()
            || !config.origin.username().is_empty()
            || config.origin.password().is_some()
            || config.account_attempts < 1
            || config.source_attempts < 1
            || config.account_attempts == i32::MAX
            || config.source_attempts == i32::MAX
            || config.attempt_window.as_secs() == 0
            || config.attempt_window.as_secs() > 86400
        {
            return Err(AuthError::InvalidInput);
        }
        let webauthn = WebauthnBuilder::new(
            config.origin.domain().ok_or(AuthError::InvalidInput)?,
            &config.origin,
        )
        .map_err(|_| AuthError::InvalidInput)?
        .rp_name("MIDAS")
        .timeout(config.challenge_lifetime)
        .build()
        .map_err(|_| AuthError::InvalidInput)?;
        let challenges = Challenges::new(config.challenge_lifetime, config.challenge_capacity)?;
        let vault = Vault::new(config.encryption_key)?;
        let passwords = Passwords::new(config.password_workers)?;
        let dummy_hash = passwords
            .hash(Zeroizing::new(hex::encode(random::<32>()?)))
            .await?;
        Ok(Self {
            pool: database.pool().clone(),
            webauthn,
            passwords,
            dummy_hash,
            vault,
            challenges,
            account_attempts: config.account_attempts,
            source_attempts: config.source_attempts,
            attempt_window: config.attempt_window.as_secs() as i64,
            last_cleanup: AtomicU64::new(0),
        })
    }

    /// Trusted provisioning only: setup or an authorized administrator must guard this call.
    pub async fn create_operator(&self, login: &str, display_name: &str) -> Result<Uuid> {
        let login = normalize_login(login)?;
        if display_name.trim().is_empty() || display_name.chars().count() > 200 {
            return Err(AuthError::InvalidInput);
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO midas.operators (id, login, display_name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(login)
            .bind(display_name)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    /// Caller must authorize enrollment for this operator; this is not an open registration endpoint.
    pub async fn begin_registration(
        &self,
        operator: Uuid,
        binding: &BrowserBinding,
    ) -> Result<Issued<CreationChallengeResponse>> {
        let account = self
            .account_by_id(operator)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        let keys = self.keys(operator).await?;
        if keys.len() >= 32 {
            return Err(AuthError::InvalidInput);
        }
        let excluded = keys.iter().map(|key| key.cred_id().clone()).collect();
        let (options, state) = self
            .webauthn
            .start_passkey_registration(
                operator,
                &account.login,
                &account.display_name,
                Some(excluded),
            )
            .map_err(|_| AuthError::Unavailable)?;
        let id = self.challenges.insert(
            binding,
            Pending {
                user: Some(operator),
                version: account.credential_version,
                login: account.login,
                ceremony: Ceremony::Register(state),
            },
        )?;
        Ok(Issued { id, options })
    }

    pub async fn finish_registration(
        &self,
        id: &str,
        binding: &BrowserBinding,
        response: &RegisterPublicKeyCredential,
    ) -> Result<Authenticated> {
        let pending = self.challenges.take(id, binding)?;
        let Ceremony::Register(state) = pending.ceremony else {
            return Err(AuthError::InvalidCredentials);
        };
        let user = pending.user.ok_or(AuthError::InvalidCredentials)?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(response, &state)
            .map_err(|_| AuthError::InvalidCredentials)?;
        let encoded = serde_json::to_string(&passkey).map_err(|_| AuthError::Unavailable)?;
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query("UPDATE midas.operators SET credential_version = credential_version + 1 WHERE id = $1 AND enabled AND credential_version = $2")
            .bind(user).bind(pending.version).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(AuthError::InvalidCredentials);
        }
        sqlx::query("INSERT INTO midas.passkeys (credential_id, operator_id, credential) VALUES ($1, $2, $3)")
            .bind(passkey.cred_id().as_slice()).bind(user).bind(encoded).execute(&mut *tx).await?;
        tx.commit().await?;
        // Registration requires user verification, so it proves a fresh passkey ceremony.
        Ok(authenticated(
            user,
            pending.version + 1,
            AuthenticationMethod::Passkey,
        ))
    }

    pub async fn begin_passkey_login(
        &self,
        login: &str,
        source: IpAddr,
        binding: &BrowserBinding,
    ) -> Result<Issued<RequestChallengeResponse>> {
        let login = normalize_login(login).map_err(|_| AuthError::InvalidCredentials)?;
        self.limit(&login, source).await?;
        let account = self.account_by_login(&login).await?;
        let keys = match &account {
            Some(a) => self.keys(a.id).await?,
            None => Vec::new(),
        };
        let (options, state) = self
            .webauthn
            .start_passkey_authentication(&keys)
            .map_err(|_| AuthError::Unavailable)?;
        let id = self.challenges.insert(
            binding,
            Pending {
                user: account.as_ref().map(|a| a.id),
                version: account.map_or(0, |a| a.credential_version),
                login,
                ceremony: Ceremony::Authenticate(state),
            },
        )?;
        Ok(Issued { id, options })
    }

    pub async fn finish_passkey_login(
        &self,
        id: &str,
        source: IpAddr,
        binding: &BrowserBinding,
        response: &PublicKeyCredential,
    ) -> Result<Authenticated> {
        let pending = self.challenges.take(id, binding)?;
        self.limit(&pending.login, source).await?;
        let Ceremony::Authenticate(state) = pending.ceremony else {
            return Err(AuthError::InvalidCredentials);
        };
        let user = pending.user.ok_or(AuthError::InvalidCredentials)?;
        let result = self
            .webauthn
            .finish_passkey_authentication(response, &state)
            .map_err(|_| AuthError::InvalidCredentials)?;
        if !result.user_verified() {
            return Err(AuthError::InvalidCredentials);
        }
        let mut tx = self.pool.begin().await?;
        let version: Option<i64> = sqlx::query_scalar(
            "SELECT credential_version FROM midas.operators WHERE id = $1 AND enabled FOR UPDATE",
        )
        .bind(user)
        .fetch_optional(&mut *tx)
        .await?;
        if version != Some(pending.version) {
            return Err(AuthError::InvalidCredentials);
        }
        let row: Option<(String, i64)> = sqlx::query_as("SELECT credential, signature_counter FROM midas.passkeys WHERE operator_id = $1 AND credential_id = $2 FOR UPDATE")
            .bind(user).bind(result.cred_id().as_slice()).fetch_optional(&mut *tx).await?;
        let (encoded, counter) = row.ok_or(AuthError::InvalidCredentials)?;
        let new_counter = i64::from(result.counter());
        if (counter > 0 || new_counter > 0) && new_counter <= counter {
            return Err(AuthError::InvalidCredentials);
        }
        let mut key: Passkey =
            serde_json::from_str(&encoded).map_err(|_| AuthError::Unavailable)?;
        key.update_credential(&result)
            .ok_or(AuthError::InvalidCredentials)?;
        sqlx::query("UPDATE midas.passkeys SET credential = $1, signature_counter = $2 WHERE credential_id = $3")
            .bind(serde_json::to_string(&key).map_err(|_| AuthError::Unavailable)?)
            .bind(new_counter).bind(result.cred_id().as_slice()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(authenticated(
            user,
            pending.version,
            AuthenticationMethod::Passkey,
        ))
    }

    /// Trusted enrollment only; keep this value on the server until possession is verified.
    pub async fn begin_fallback_enrollment(&self, operator: Uuid) -> Result<TotpEnrollment> {
        let account = self
            .account_by_id(operator)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        Ok(TotpEnrollment {
            operator,
            version: account.credential_version,
            secret: Zeroizing::new(random::<20>()?.to_vec()),
            expires: Instant::now() + Duration::from_secs(300),
            login: account.login,
        })
    }

    pub async fn finish_fallback_enrollment(
        &self,
        enrollment: TotpEnrollment,
        password: Zeroizing<String>,
        code: &str,
    ) -> Result<()> {
        if enrollment.expires <= Instant::now() {
            return Err(AuthError::InvalidCredentials);
        }
        let step = totp(enrollment.secret.to_vec(), &enrollment.login)?
            .check(code, now()?)
            .ok_or(AuthError::InvalidCredentials)?;
        let hash = self.passwords.hash(password).await?;
        let sealed = self.vault.seal(enrollment.operator, &enrollment.secret)?;
        let changed = sqlx::query("UPDATE midas.operators SET password_hash = $1, totp_secret = $2, totp_last_step = $3, credential_version = credential_version + 1 WHERE id = $4 AND enabled AND credential_version = $5")
            .bind(hash).bind(sealed).bind(step as i64).bind(enrollment.operator).bind(enrollment.version)
            .execute(&self.pool).await?.rows_affected();
        if changed != 1 {
            return Err(AuthError::InvalidCredentials);
        }
        Ok(())
    }

    pub async fn login_password_totp(
        &self,
        login: &str,
        password: Zeroizing<String>,
        code: &str,
        source: IpAddr,
    ) -> Result<Authenticated> {
        let login = normalize_login(login).map_err(|_| AuthError::InvalidCredentials)?;
        self.limit(&login, source).await?;
        let account = self.account_by_login(&login).await?;
        let hash = account
            .as_ref()
            .and_then(|a| a.password_hash.clone())
            .unwrap_or_else(|| self.dummy_hash.clone());
        let verified = self.passwords.verify(password, hash.clone()).await?;
        let account = account
            .filter(|a| a.password_hash.is_some() && a.totp_secret.is_some())
            .ok_or(AuthError::InvalidCredentials)?;
        if !verified {
            return Err(AuthError::InvalidCredentials);
        }
        let seed = self.vault.open(
            account.id,
            account
                .totp_secret
                .as_deref()
                .ok_or(AuthError::InvalidCredentials)?,
        )?;
        let step = totp(seed.to_vec(), &login)?
            .check(code, now()?)
            .ok_or(AuthError::InvalidCredentials)?;
        let changed = sqlx::query("UPDATE midas.operators SET totp_last_step = $1 WHERE id = $2 AND enabled AND credential_version = $3 AND password_hash = $4 AND totp_last_step < $1")
            .bind(step as i64).bind(account.id).bind(account.credential_version).bind(hash)
            .execute(&self.pool).await?.rows_affected();
        if changed != 1 {
            return Err(AuthError::InvalidCredentials);
        }
        Ok(authenticated(
            account.id,
            account.credential_version,
            AuthenticationMethod::PasswordTotp,
        ))
    }

    /// Requires trusted setup or fresh authorized authentication; invalidates previous codes.
    pub async fn replace_recovery_codes(&self, operator: Uuid) -> Result<Vec<Zeroizing<String>>> {
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query("UPDATE midas.operators SET credential_version = credential_version + 1 WHERE id = $1 AND enabled")
            .bind(operator).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(AuthError::InvalidCredentials);
        }
        sqlx::query("DELETE FROM midas.recovery_codes WHERE operator_id = $1")
            .bind(operator)
            .execute(&mut *tx)
            .await?;
        let mut codes = Vec::new();
        for _ in 0..10 {
            let code = Zeroizing::new(hex::encode(random::<32>()?));
            sqlx::query("INSERT INTO midas.recovery_codes (operator_id, digest) VALUES ($1, $2)")
                .bind(operator)
                .bind(digest(b"recovery-code", code.as_bytes()))
                .execute(&mut *tx)
                .await?;
            codes.push(code);
        }
        tx.commit().await?;
        Ok(codes)
    }

    pub async fn login_recovery(
        &self,
        login: &str,
        code: Zeroizing<String>,
        source: IpAddr,
    ) -> Result<Authenticated> {
        let login = normalize_login(login).map_err(|_| AuthError::InvalidCredentials)?;
        self.limit(&login, source).await?;
        if code.len() != 64 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AuthError::InvalidCredentials);
        }
        let mut tx = self.pool.begin().await?;
        let account: Option<(Uuid, i64)> = sqlx::query_as("SELECT id, credential_version FROM midas.operators WHERE login = $1 AND enabled FOR UPDATE")
            .bind(login).fetch_optional(&mut *tx).await?;
        let (user, version) = account.ok_or(AuthError::InvalidCredentials)?;
        let canonical = Zeroizing::new(code.to_ascii_lowercase());
        let consumed =
            sqlx::query("DELETE FROM midas.recovery_codes WHERE operator_id = $1 AND digest = $2")
                .bind(user)
                .bind(digest(b"recovery-code", canonical.as_bytes()))
                .execute(&mut *tx)
                .await?
                .rows_affected();
        if consumed != 1 {
            return Err(AuthError::InvalidCredentials);
        }
        tx.commit().await?;
        Ok(authenticated(
            user,
            version,
            AuthenticationMethod::RecoveryCode,
        ))
    }

    pub(crate) async fn profile(&self, operator: Uuid) -> Result<Option<Profile>> {
        Ok(sqlx::query_as(
            "SELECT login, display_name, platform_admin FROM midas.operators WHERE id = $1 AND enabled",
        )
        .bind(operator)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub(crate) async fn operator_for_login(&self, login: &str) -> Result<Option<Uuid>> {
        let Ok(login) = normalize_login(login) else {
            return Ok(None);
        };
        Ok(
            sqlx::query_scalar("SELECT id FROM midas.operators WHERE login = $1")
                .bind(login)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    async fn keys(&self, operator: Uuid) -> Result<Vec<Passkey>> {
        let rows: Vec<String> = sqlx::query_scalar("SELECT credential FROM midas.passkeys WHERE operator_id = $1 ORDER BY credential_id LIMIT 33")
            .bind(operator).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| serde_json::from_str(&row).map_err(|_| AuthError::Unavailable))
            .collect()
    }

    async fn account_by_id(&self, id: Uuid) -> Result<Option<Account>> {
        Ok(sqlx::query_as("SELECT id, login, display_name, credential_version, password_hash, totp_secret FROM midas.operators WHERE id = $1 AND enabled")
            .bind(id).fetch_optional(&self.pool).await?)
    }

    async fn account_by_login(&self, login: &str) -> Result<Option<Account>> {
        Ok(sqlx::query_as("SELECT id, login, display_name, credential_version, password_hash, totp_secret FROM midas.operators WHERE login = $1 AND enabled")
            .bind(login).fetch_optional(&self.pool).await?)
    }

    async fn limit(&self, login: &str, source: IpAddr) -> Result<()> {
        let current = now()?;
        let last = self.last_cleanup.load(Ordering::Relaxed);
        if current.saturating_sub(last) >= 60
            && self
                .last_cleanup
                .compare_exchange(last, current, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            sqlx::query(
                "DELETE FROM midas.authentication_attempts WHERE expires_at <= clock_timestamp()",
            )
            .execute(&self.pool)
            .await?;
        }
        for (bucket, maximum) in [
            (
                digest(b"source-ip", source.to_canonical().to_string().as_bytes()),
                self.source_attempts,
            ),
            (digest(b"account", login.as_bytes()), self.account_attempts),
        ] {
            let attempt: Option<i32> = sqlx::query_scalar(
                "INSERT INTO midas.authentication_attempts AS a (bucket, attempts, expires_at) VALUES ($1, 1, clock_timestamp() + make_interval(secs => $3)) ON CONFLICT (bucket) DO UPDATE SET attempts = CASE WHEN a.expires_at <= clock_timestamp() THEN 1 ELSE a.attempts + 1 END, expires_at = CASE WHEN a.expires_at <= clock_timestamp() THEN clock_timestamp() + make_interval(secs => $3) ELSE a.expires_at END WHERE a.expires_at <= clock_timestamp() OR a.attempts < $2 RETURNING attempts")
                .bind(bucket).bind(maximum).bind(self.attempt_window as f64).fetch_optional(&self.pool).await?;
            if attempt.is_none() {
                return Err(AuthError::RateLimited);
            }
        }
        Ok(())
    }
}

fn normalize_login(login: &str) -> Result<String> {
    let login = login.trim().to_ascii_lowercase();
    if login.is_empty()
        || login.len() > 254
        || !login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".@_+-".contains(&b))
    {
        return Err(AuthError::InvalidInput);
    }
    Ok(login)
}

fn totp(secret: Vec<u8>, login: &str) -> Result<Totp> {
    Builder::new()
        .with_secret(secret)
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_step_duration(30)
        .with_skew(1)
        .with_account_name(login)
        .with_issuer(Some("MIDAS"))
        .build()
        .map_err(|_| AuthError::Unavailable)
}

fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| AuthError::Unavailable)
}

fn authenticated(operator: Uuid, version: i64, method: AuthenticationMethod) -> Authenticated {
    Authenticated {
        operator,
        version,
        method,
        verified_at: SystemTime::now(),
    }
}
