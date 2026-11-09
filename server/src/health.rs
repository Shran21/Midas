use crate::database::driver as sqlx;
use midas_protocol::{
    HealthCheck, HealthReport, HealthState, MAX_HEALTH_CHECKS, MAX_HEALTH_SUBJECTS, MAX_HEALTH_TEXT,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    auth::sessions::{SessionToken, Sessions},
    authorization::{AccessError, Permission, Target},
    enrollment::{AgentIdentity, Enrollment},
};

#[derive(Debug, Serialize)]
pub struct HealthView {
    /// What the agent announced in its last handshake, such as `health.encryption`.
    pub capabilities: Vec<String>,
    pub reported_at: Option<i64>,
    pub checks: Vec<HealthEntry>,
}

#[derive(Debug, Serialize)]
pub struct HealthEntry {
    pub id: String,
    pub state: String,
    pub code: String,
    pub summary: String,
    pub subjects: Vec<String>,
    /// When the state or reason last changed.
    pub changed_at: i64,
}
crate::database::postgres_row!(HealthEntry {
    id,
    state,
    code,
    summary,
    subjects,
    changed_at
});

fn identifier(text: &str, longest: usize) -> bool {
    (1..=longest).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

fn text(text: &str) -> bool {
    !text.contains('\0') && text.chars().count() <= MAX_HEALTH_TEXT
}

fn state(check: &HealthCheck) -> Option<&'static str> {
    match HealthState::try_from(check.state).ok()? {
        HealthState::Ok => Some("ok"),
        HealthState::Warning => Some("warning"),
        HealthState::Critical => Some("critical"),
        HealthState::Unavailable => Some("unavailable"),
        HealthState::Unspecified => None,
    }
}

/// Valid checks only, the first of each id; an invalid check is skipped, not fatal.
fn valid(report: &HealthReport) -> Vec<(&HealthCheck, &'static str)> {
    let mut seen = std::collections::HashSet::new();
    report
        .checks
        .iter()
        .filter_map(|check| {
            let state = state(check)?;
            let ok = identifier(&check.id, 16)
                && identifier(&check.code, 32)
                && text(&check.summary)
                && check.subjects.len() <= MAX_HEALTH_SUBJECTS
                && check.subjects.iter().all(|subject| text(subject));
            (ok && seen.insert(check.id.as_str())).then_some((check, state))
        })
        .collect()
}

impl Enrollment {
    /// Stores what the agent announced; a later handshake replaces it.
    pub async fn record_capabilities(
        &self,
        identity: AgentIdentity,
        capabilities: &[String],
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.tenant_tx(identity.tenant).await?;
        sqlx::query("UPDATE midas.devices SET capabilities = $2 WHERE id = $1")
            .bind(identity.device)
            .bind(capabilities)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    /// Replaces the device's check results; `changed_at` moves only when state or reason changes.
    pub async fn apply_health(
        &self,
        identity: AgentIdentity,
        report: &HealthReport,
    ) -> Result<(), sqlx::Error> {
        if report.checks.len() > MAX_HEALTH_CHECKS {
            return Err(sqlx::Error::Protocol("health report too large".into()));
        }
        let checks = valid(report);
        let mut tx = self.tenant_tx(identity.tenant).await?;
        for (check, state) in &checks {
            sqlx::query("INSERT INTO midas.device_health (tenant_id, device_id, check_id, state, code, summary, subjects) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (tenant_id, device_id, check_id) DO UPDATE SET changed_at = CASE WHEN (midas.device_health.state, midas.device_health.code) IS DISTINCT FROM (EXCLUDED.state, EXCLUDED.code) THEN clock_timestamp() ELSE midas.device_health.changed_at END, state = EXCLUDED.state, code = EXCLUDED.code, summary = EXCLUDED.summary, subjects = EXCLUDED.subjects")
                .bind(identity.tenant).bind(identity.device).bind(&check.id).bind(*state)
                .bind(&check.code).bind(&check.summary).bind(&check.subjects)
                .execute(&mut *tx).await?;
        }
        crate::alerts::reconcile_health(&mut tx, identity.tenant, identity.device, &checks).await?;
        let ids: Vec<&str> = checks.iter().map(|(check, _)| check.id.as_str()).collect();
        sqlx::query(
            "DELETE FROM midas.device_health WHERE device_id = $1 AND NOT (check_id = ANY($2))",
        )
        .bind(identity.device)
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE midas.devices SET health_reported_at = clock_timestamp() WHERE id = $1",
        )
        .bind(identity.device)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.changed(identity.tenant);
        Ok(())
    }

    pub async fn health(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<HealthView, AccessError> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let target = Target {
            tenant,
            device: Some(device),
        };
        self.authorizer
            .authorize_view(&identity, target, Permission::DeviceRead)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let device_row: Option<(Vec<String>, Option<i64>)> = sqlx::query_as(
            "SELECT capabilities, extract(epoch FROM health_reported_at)::bigint FROM midas.devices WHERE id = $1",
        )
        .bind(device)
        .fetch_optional(&mut *tx)
        .await?;
        let (capabilities, reported_at) = device_row.ok_or(AccessError::Denied)?;
        let checks = sqlx::query_as("SELECT check_id AS id, state, code, summary, subjects, extract(epoch FROM changed_at)::bigint AS changed_at FROM midas.device_health WHERE device_id = $1 ORDER BY check_id")
            .bind(device).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(HealthView {
            capabilities,
            reported_at,
            checks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(id: &str, state: HealthState, code: &str) -> HealthCheck {
        HealthCheck {
            id: id.into(),
            state: state as i32,
            code: code.into(),
            summary: "summary".into(),
            subjects: Vec::new(),
        }
    }

    #[test]
    fn invalid_and_repeated_checks_are_skipped() {
        let mut nul = check("storage", HealthState::Ok, "healthy");
        nul.summary = "bad\0".into();
        let mut crowded = check("services", HealthState::Critical, "stopped");
        crowded.subjects = vec!["x".into(); MAX_HEALTH_SUBJECTS + 1];
        let report = HealthReport {
            sequence: 1,
            checks: vec![
                check("updates", HealthState::Warning, "reboot_pending"),
                check("updates", HealthState::Ok, "current"),
                check("Antivirus", HealthState::Ok, "protected"),
                check("encryption", HealthState::Unspecified, "encrypted"),
                check("antivirus", HealthState::Unavailable, "not supported"),
                nul,
                crowded,
                check("antivirus", HealthState::Unavailable, "not_supported"),
            ],
        };
        let kept: Vec<_> = valid(&report)
            .into_iter()
            .map(|(check, state)| (check.id.as_str(), state, check.code.as_str()))
            .collect();
        assert_eq!(
            kept,
            [
                ("updates", "warning", "reboot_pending"),
                ("antivirus", "unavailable", "not_supported"),
            ]
        );
    }
}
