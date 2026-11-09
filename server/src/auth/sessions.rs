use crate::database::driver as sqlx;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hmac::{Hmac, KeyInit, Mac};
use serde::Serialize;
use sha2::Sha256;
use sqlx::PgPool;
use tokio::sync::broadcast;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{
    AuthError, Authenticated, AuthenticationMethod, Result,
    crypto::{digest, random},
};
use crate::database::Database;

#[derive(Clone)]
pub struct SessionToken(Zeroizing<[u8; 32]>);

impl SessionToken {
    pub fn parse(value: &str) -> Result<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(AuthError::InvalidCredentials);
        }
        let mut bytes = Zeroizing::new([0; 32]);
        hex::decode_to_slice(value, bytes.as_mut()).map_err(|_| AuthError::InvalidCredentials)?;
        if *bytes == [0; 32] {
            return Err(AuthError::InvalidCredentials);
        }
        Ok(Self(bytes))
    }

    pub fn encode(&self) -> Zeroizing<String> {
        Zeroizing::new(hex::encode(self.0.as_slice()))
    }

    fn digest(&self) -> Vec<u8> {
        digest(b"browser-session", self.0.as_slice())
    }

    fn csrf_mac(&self) -> Result<Hmac<Sha256>> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.0.as_slice())
            .map_err(|_| AuthError::Unavailable)?;
        mac.update(b"midas/browser-session/csrf/v1");
        Ok(mac)
    }

    pub fn csrf(&self) -> Result<Zeroizing<String>> {
        Ok(Zeroizing::new(hex::encode(
            self.csrf_mac()?.finalize().into_bytes(),
        )))
    }

    pub fn verify_csrf(&self, value: &str) -> Result<()> {
        if value.len() != 64 {
            return Err(AuthError::InvalidCredentials);
        }
        let mut bytes = Zeroizing::new([0; 32]);
        hex::decode_to_slice(value, bytes.as_mut()).map_err(|_| AuthError::InvalidCredentials)?;
        self.csrf_mac()?
            .verify_slice(bytes.as_slice())
            .map_err(|_| AuthError::InvalidCredentials)
    }
}

pub struct SessionConfig {
    pub absolute_lifetime: Duration,
    pub idle_lifetime: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            absolute_lifetime: Duration::from_secs(43_200),
            idle_lifetime: Duration::from_secs(1_800),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionInfo {
    pub id: Uuid,
    pub operator_id: Uuid,
    pub credential_version: i64,
    pub authentication_method: AuthenticationMethod,
    pub authenticated_at: i64,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
}

struct Row {
    id: Uuid,
    operator_id: Uuid,
    credential_version: i64,
    authentication_method: String,
    authenticated_at: i64,
    created_at: i64,
    last_seen_at: i64,
    expires_at: i64,
}
crate::database::postgres_row!(Row {
    id,
    operator_id,
    credential_version,
    authentication_method,
    authenticated_at,
    created_at,
    last_seen_at,
    expires_at
});

impl TryFrom<Row> for SessionInfo {
    type Error = AuthError;
    fn try_from(row: Row) -> Result<Self> {
        let authentication_method = match row.authentication_method.as_str() {
            "passkey" => AuthenticationMethod::Passkey,
            "password_totp" => AuthenticationMethod::PasswordTotp,
            "recovery_code" => AuthenticationMethod::RecoveryCode,
            "directory" => AuthenticationMethod::Directory,
            _ => return Err(AuthError::Unavailable),
        };
        Ok(Self {
            id: row.id,
            operator_id: row.operator_id,
            credential_version: row.credential_version,
            authentication_method,
            authenticated_at: row.authenticated_at,
            created_at: row.created_at,
            last_seen_at: row.last_seen_at,
            expires_at: row.expires_at,
        })
    }
}

pub struct IssuedSession {
    pub token: SessionToken,
    pub info: SessionInfo,
}

pub struct Sessions {
    pool: PgPool,
    config: SessionConfig,
    revoked: broadcast::Sender<Uuid>,
    last_cleanup: AtomicU64,
}

impl Sessions {
    pub fn new(database: &Database, config: SessionConfig) -> Result<Self> {
        if config.idle_lifetime.as_secs() == 0
            || config.absolute_lifetime.as_secs() == 0
            || config.idle_lifetime > config.absolute_lifetime
            || config.absolute_lifetime.as_secs() > 31_536_000
        {
            return Err(AuthError::InvalidInput);
        }
        let (revoked, _) = broadcast::channel(256);
        Ok(Self {
            pool: database.pool().clone(),
            config,
            revoked,
            last_cleanup: AtomicU64::new(0),
        })
    }

    pub fn lifetime_seconds(&self) -> u64 {
        self.config.absolute_lifetime.as_secs()
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Uuid> {
        self.revoked.subscribe()
    }

    pub async fn issue(
        &self,
        proof: Authenticated,
        replace: Option<&SessionToken>,
    ) -> Result<IssuedSession> {
        if proof
            .verified_at()
            .elapsed()
            .map_err(|_| AuthError::InvalidCredentials)?
            > Duration::from_secs(60)
        {
            return Err(AuthError::InvalidCredentials);
        }
        self.cleanup().await?;
        let token = SessionToken(Zeroizing::new(random()?));
        let id = Uuid::new_v4();
        let method = match proof.method() {
            AuthenticationMethod::Passkey => "passkey",
            AuthenticationMethod::PasswordTotp => "password_totp",
            AuthenticationMethod::RecoveryCode => "recovery_code",
            AuthenticationMethod::Directory => "directory",
        };
        let verified_at = proof
            .verified_at()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::Unavailable)?
            .as_secs_f64();
        let mut tx = self.pool.begin().await?;
        let version: Option<i64> = sqlx::query_scalar(
            "SELECT credential_version FROM midas.operators WHERE id = $1 AND enabled FOR SHARE",
        )
        .bind(proof.operator_id())
        .fetch_optional(&mut *tx)
        .await?;
        if version != Some(proof.credential_version()) {
            return Err(AuthError::InvalidCredentials);
        }
        sqlx::query("INSERT INTO midas.browser_sessions (id, token_hash, operator_id, credential_version, authentication_method, authenticated_at, expires_at, idle_expires_at, idle_seconds) VALUES ($1, $2, $3, $4, $5, to_timestamp($6), clock_timestamp() + make_interval(secs => $7), clock_timestamp() + make_interval(secs => $8), $9)")
            .bind(id).bind(token.digest()).bind(proof.operator_id()).bind(proof.credential_version()).bind(method).bind(verified_at)
            .bind(self.config.absolute_lifetime.as_secs_f64()).bind(self.config.idle_lifetime.as_secs_f64())
            .bind(self.config.idle_lifetime.as_secs() as i32).execute(&mut *tx).await?;
        let previous: Option<Uuid> = if let Some(old) = replace {
            sqlx::query_scalar(
                "DELETE FROM midas.browser_sessions WHERE token_hash = $1 RETURNING id",
            )
            .bind(old.digest())
            .fetch_optional(&mut *tx)
            .await?
        } else {
            None
        };
        tx.commit().await?;
        if let Some(previous) = previous {
            let _ = self.revoked.send(previous);
        }
        let info = self.validate(&token, false).await?;
        Ok(IssuedSession { token, info })
    }

    pub async fn validate(&self, token: &SessionToken, activity: bool) -> Result<SessionInfo> {
        let row: Option<Row> = sqlx::query_as(
            "WITH valid AS (SELECT s.* FROM midas.browser_sessions s JOIN midas.operators o ON o.id = s.operator_id WHERE s.token_hash = $1 AND o.enabled AND o.credential_version = s.credential_version AND s.expires_at > clock_timestamp() AND s.idle_expires_at > clock_timestamp()), touch AS (UPDATE midas.browser_sessions s SET last_seen_at = clock_timestamp(), idle_expires_at = clock_timestamp() + make_interval(secs => s.idle_seconds) FROM valid v WHERE s.id = v.id AND $2 AND s.last_seen_at < clock_timestamp() - make_interval(secs => greatest(1, least(60, s.idle_seconds / 4)))) SELECT id, operator_id, credential_version, authentication_method, extract(epoch from authenticated_at)::bigint AS authenticated_at, extract(epoch from created_at)::bigint AS created_at, extract(epoch from last_seen_at)::bigint AS last_seen_at, extract(epoch from expires_at)::bigint AS expires_at FROM valid")
            .bind(token.digest()).bind(activity).fetch_optional(&self.pool).await?;
        row.ok_or(AuthError::InvalidCredentials)?.try_into()
    }

    pub async fn list(&self, current: &SessionToken) -> Result<Vec<SessionInfo>> {
        let current = self.validate(current, true).await?;
        let rows: Vec<Row> = sqlx::query_as("SELECT id, operator_id, credential_version, authentication_method, extract(epoch from authenticated_at)::bigint AS authenticated_at, extract(epoch from created_at)::bigint AS created_at, extract(epoch from last_seen_at)::bigint AS last_seen_at, extract(epoch from expires_at)::bigint AS expires_at FROM midas.browser_sessions WHERE operator_id = $1 AND credential_version = $2 AND expires_at > clock_timestamp() AND idle_expires_at > clock_timestamp() ORDER BY created_at DESC LIMIT 100")
            .bind(current.operator_id).bind(current.credential_version).fetch_all(&self.pool).await?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn revoke(&self, current: &SessionToken, id: Uuid) -> Result<bool> {
        let current = self.validate(current, false).await?;
        let removed =
            sqlx::query("DELETE FROM midas.browser_sessions WHERE id = $1 AND operator_id = $2")
                .bind(id)
                .bind(current.operator_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                == 1;
        if removed {
            let _ = self.revoked.send(id);
        }
        Ok(removed)
    }

    async fn cleanup(&self) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::Unavailable)?
            .as_secs();
        let last = self.last_cleanup.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= 60
            && self
                .last_cleanup
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            sqlx::query("DELETE FROM midas.browser_sessions WHERE expires_at <= clock_timestamp() OR idle_expires_at <= clock_timestamp()")
                .execute(&self.pool).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_is_bound_to_the_secret_and_tokens_are_strict() {
        let a = SessionToken(Zeroizing::new(random().unwrap()));
        let b = SessionToken(Zeroizing::new(random().unwrap()));
        let csrf = a.csrf().unwrap();
        a.verify_csrf(&csrf).unwrap();
        assert!(b.verify_csrf(&csrf).is_err());
        assert!(a.verify_csrf(&a.encode()).is_err());
        assert!(a.verify_csrf("invalid").is_err());
        SessionToken::parse(&a.encode())
            .unwrap()
            .verify_csrf(&csrf)
            .unwrap();
        for bad in [
            "x".repeat(64),
            "0".repeat(64),
            "a".repeat(63),
            "A".repeat(64),
        ] {
            assert!(SessionToken::parse(&bad).is_err());
        }
    }
}
