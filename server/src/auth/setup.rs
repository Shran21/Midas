use crate::database::driver as sqlx;
use std::time::Duration;

use uuid::Uuid;
use webauthn_rs::prelude::{CreationChallengeResponse, RegisterPublicKeyCredential};
use zeroize::Zeroizing;

use super::{
    AuthError, AuthService, Authenticated, BrowserBinding, Issued, Result, digest, random,
};
use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    database::Database,
};

fn token_digest(token: &str) -> Result<Vec<u8>> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(AuthError::InvalidCredentials);
    }
    Ok(digest(
        b"setup-token",
        token.to_ascii_lowercase().as_bytes(),
    ))
}

/// Host-side only: issues the single first-administrator setup authorization and voids older unused ones.
pub async fn issue_token(database: &Database, lifetime: Duration) -> Result<Zeroizing<String>> {
    if lifetime.is_zero() || lifetime > Duration::from_secs(86_400) {
        return Err(AuthError::InvalidInput);
    }
    let mut tx = database.pool().begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('midas.setup', 0))")
        .execute(&mut *tx)
        .await?;
    let administered: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM midas.operators WHERE platform_admin AND enabled)",
    )
    .fetch_one(&mut *tx)
    .await?;
    if administered {
        return Err(AuthError::Conflict);
    }
    sqlx::query("UPDATE midas.setup_tokens SET used_at = clock_timestamp() WHERE used_at IS NULL")
        .execute(&mut *tx)
        .await?;
    let token = Zeroizing::new(hex::encode(random::<32>()?));
    sqlx::query("INSERT INTO midas.setup_tokens (digest, expires_at) VALUES ($1, clock_timestamp() + make_interval(secs => $2))")
        .bind(token_digest(&token)?).bind(lifetime.as_secs_f64()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(token)
}

impl AuthService {
    /// Starts or resumes passkey registration for the first administrator named by an unused setup authorization.
    pub async fn begin_setup(
        &self,
        token: &str,
        login: &str,
        display_name: &str,
        binding: &BrowserBinding,
    ) -> Result<Issued<CreationChallengeResponse>> {
        let digest = token_digest(token)?;
        let mut tx = self.pool.begin().await?;
        let row: Option<Option<Uuid>> = sqlx::query_scalar("SELECT operator_id FROM midas.setup_tokens WHERE digest = $1 AND used_at IS NULL AND expires_at > clock_timestamp() FOR UPDATE")
            .bind(&digest).fetch_optional(&mut *tx).await?;
        let operator = match row.ok_or(AuthError::InvalidCredentials)? {
            Some(operator) => operator,
            None => {
                let operator = self.create_operator(login, display_name).await?;
                sqlx::query("UPDATE midas.setup_tokens SET operator_id = $2 WHERE digest = $1")
                    .bind(&digest)
                    .bind(operator)
                    .execute(&mut *tx)
                    .await?;
                operator
            }
        };
        tx.commit().await?;
        self.begin_registration(operator, binding).await
    }

    /// Consumes the setup authorization under a row lock, so a second completion always fails.
    pub async fn finish_setup(
        &self,
        token: &str,
        id: &str,
        binding: &BrowserBinding,
        response: &RegisterPublicKeyCredential,
    ) -> Result<Authenticated> {
        let digest = token_digest(token)?;
        let mut tx = self.pool.begin().await?;
        let operator: Uuid = sqlx::query_scalar("SELECT operator_id FROM midas.setup_tokens WHERE digest = $1 AND used_at IS NULL AND expires_at > clock_timestamp() AND operator_id IS NOT NULL FOR UPDATE")
            .bind(&digest).fetch_optional(&mut *tx).await?
            .ok_or(AuthError::InvalidCredentials)?;
        let proof = self.finish_registration(id, binding, response).await?;
        if proof.operator_id() != operator {
            return Err(AuthError::InvalidCredentials);
        }
        sqlx::query("UPDATE midas.setup_tokens SET used_at = clock_timestamp() WHERE digest = $1")
            .bind(&digest)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE midas.operators SET platform_admin = true WHERE id = $1")
            .bind(operator)
            .execute(&mut *tx)
            .await?;
        let actor = Actor {
            operator: Some(operator),
            session: None,
        };
        let mut event = Event::new(Action::SetupComplete, Outcome::Success, actor);
        event.subject = Some(operator);
        audit::append_platform(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(proof)
    }
}
