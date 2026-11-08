use std::time::Duration;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};
use midas_protocol::{HealthCheck, policy};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    auth::sessions::{SessionToken, Sessions},
    authorization::{AccessError, Permission, Target},
    enrollment::Enrollment,
};

type Result<T> = std::result::Result<T, AccessError>;

pub const OFFLINE: &str = "device.offline";
/// A condition that returns this soon after resolving reopens its alert without a new notification.
const REOPEN_SECONDS: f64 = 600.0;
const MAX_ATTEMPTS: i32 = 6;
const BATCH: i64 = 20;
const MAX_RECIPIENTS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Critical,
}

impl Severity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Raised {
    /// A new alert; its notifications are queued.
    New(Uuid),
    Updated(Uuid),
    Reopened(Uuid),
}

/// Opens an alert or folds the repeat into the open or recently resolved one.
pub(crate) async fn raise(
    tx: &mut PgConnection,
    tenant: Uuid,
    device: Uuid,
    (kind, severity): (&str, Severity),
    summary: &str,
) -> std::result::Result<Raised, sqlx::Error> {
    let summary: String = summary.chars().filter(|c| *c != '\0').take(512).collect();
    let open: Option<Uuid> = sqlx::query_scalar("UPDATE midas.alerts SET last_seen_at = clock_timestamp(), severity = $3, summary = $4 WHERE device_id = $1 AND kind = $2 AND resolved_at IS NULL RETURNING id")
        .bind(device).bind(kind).bind(severity.as_str()).bind(&summary)
        .fetch_optional(&mut *tx).await?;
    if let Some(id) = open {
        return Ok(Raised::Updated(id));
    }
    let reopened: Option<Uuid> = sqlx::query_scalar("UPDATE midas.alerts SET resolved_at = NULL, last_seen_at = clock_timestamp(), occurrences = occurrences + 1, severity = $3, summary = $4 WHERE id = (SELECT id FROM midas.alerts WHERE device_id = $1 AND kind = $2 AND resolved_at > clock_timestamp() - make_interval(secs => $5) ORDER BY resolved_at DESC LIMIT 1) RETURNING id")
        .bind(device).bind(kind).bind(severity.as_str()).bind(&summary).bind(REOPEN_SECONDS)
        .fetch_optional(&mut *tx).await?;
    if let Some(id) = reopened {
        return Ok(Raised::Reopened(id));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO midas.alerts (tenant_id, id, device_id, kind, severity, summary) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(tenant).bind(id).bind(device).bind(kind).bind(severity.as_str()).bind(&summary)
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO midas.alert_notifications (tenant_id, id, alert_id, recipient) SELECT $1, gen_random_uuid(), $2, r FROM midas.alert_recipients, unnest(emails) AS r")
        .bind(tenant).bind(id).execute(&mut *tx).await?;
    Ok(Raised::New(id))
}

/// Opens alerts for warning and critical checks and resolves every other health alert.
pub(crate) async fn reconcile_health(
    tx: &mut PgConnection,
    tenant: Uuid,
    device: Uuid,
    checks: &[(&HealthCheck, &'static str)],
) -> std::result::Result<(), sqlx::Error> {
    let mut active = Vec::new();
    for (check, state) in checks {
        let severity = match *state {
            "warning" => Severity::Warning,
            "critical" => Severity::Critical,
            _ => continue,
        };
        let kind = format!("health.{}", check.id);
        raise(tx, tenant, device, (&kind, severity), &check.summary).await?;
        active.push(kind);
    }
    sqlx::query("UPDATE midas.alerts SET resolved_at = clock_timestamp() WHERE device_id = $1 AND kind LIKE 'health.%' AND resolved_at IS NULL AND NOT (kind = ANY($2))")
        .bind(device).bind(&active).execute(&mut *tx).await?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    Starttls,
    Tls,
    /// Plain SMTP, accepted only for a relay on this machine.
    None,
}

pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurity,
    pub credentials: Option<(String, Zeroizing<String>)>,
    pub from: String,
}

/// Sends queued alert notifications; without SMTP they stay queued.
pub enum Mailer {
    Disabled,
    Smtp {
        transport: Box<AsyncSmtpTransport<Tokio1Executor>>,
        from: Mailbox,
    },
}

impl Mailer {
    pub fn smtp(settings: &SmtpSettings) -> std::result::Result<Self, String> {
        let from: Mailbox = settings
            .from
            .parse()
            .map_err(|_| "invalid SMTP sender address")?;
        let tls = || {
            TlsParameters::new(settings.host.clone()).map_err(|error| format!("SMTP TLS: {error}"))
        };
        let tls = match settings.security {
            SmtpSecurity::Starttls => Tls::Required(tls()?),
            SmtpSecurity::Tls => Tls::Wrapper(tls()?),
            SmtpSecurity::None => Tls::None,
        };
        let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.host)
            .port(settings.port)
            .tls(tls)
            .timeout(Some(Duration::from_secs(30)));
        if let Some((user, password)) = &settings.credentials {
            builder = builder.credentials(Credentials::new(user.clone(), password.to_string()));
        }
        Ok(Self::Smtp {
            transport: Box::new(builder.build()),
            from,
        })
    }
}

type DueRow = (
    Uuid,
    String,
    i32,
    String,
    String,
    String,
    i64,
    String,
    String,
);

fn backoff(attempts: i32) -> f64 {
    match attempts {
        ..=1 => 60.0,
        2 => 300.0,
        3 => 900.0,
        4 => 3600.0,
        _ => 14_400.0,
    }
}

fn label(kind: &str) -> (&'static str, &'static str) {
    match kind {
        OFFLINE => ("Az eszköz nem elérhető", "Device offline"),
        "health.services" => ("Leállt szolgáltatás", "Stopped service"),
        "health.updates" => ("Frissítési probléma", "Update problem"),
        "health.antivirus" => ("Vírusvédelmi probléma", "Antivirus problem"),
        "health.encryption" => ("Titkosítási probléma", "Encryption problem"),
        "health.storage" => ("Tárolóprobléma", "Storage problem"),
        _ => ("Riasztás", "Alert"),
    }
}

fn message(from: &Mailbox, recipient: &str, row: &DueRow) -> Option<Message> {
    let (_, _, _, kind, severity, summary, opened, device, tenant) = row;
    let (hu, en) = label(kind);
    let (severity_hu, severity_en) = if severity == "critical" {
        ("Kritikus", "Critical")
    } else {
        ("Figyelmeztetés", "Warning")
    };
    let at = time::OffsetDateTime::from_unix_timestamp(*opened).ok()?;
    let opened = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute()
    );
    let body = format!(
        "{severity_hu}: {hu}\nÜgyfél: {tenant}\nEszköz: {device}\nRészletek: {summary}\nKezdete (UTC): {opened}\n\
         A riasztást a MIDAS adminpaneljén lehet nyugtázni.\n\n\
         {severity_en}: {en}\nCustomer: {tenant}\nDevice: {device}\nDetails: {summary}\nStarted (UTC): {opened}\n\
         Acknowledge the alert in the MIDAS admin panel.\n"
    );
    Message::builder()
        .from(from.clone())
        .to(recipient.parse().ok()?)
        .subject(format!("[MIDAS] {severity_en}: {device} - {en}"))
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .ok()
}

/// A simple, strict address check; lettre parses it again before sending.
fn valid_email(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    (3..=254).contains(&address.len())
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && address
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '<' | '>' | ',' | ';' | '"' | '(' | ')'))
        && address.parse::<lettre::Address>().is_ok()
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AlertView {
    pub id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub kind: String,
    pub severity: String,
    pub summary: String,
    pub opened_at: i64,
    pub last_seen_at: i64,
    pub occurrences: i32,
    pub resolved_at: Option<i64>,
    pub acknowledged_at: Option<i64>,
    pub acknowledged_by: Option<Uuid>,
    pub notifications_sent: i64,
    pub notifications_pending: i64,
    pub notifications_failed: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Recipients {
    pub emails: Vec<String>,
}

impl Enrollment {
    /// Opens offline alerts after the device's policy delay and resolves them once it is back.
    pub async fn alert_sweep(&self) -> std::result::Result<(usize, u64), sqlx::Error> {
        let (mut opened, mut resolved) = (0, 0);
        for tenant in self.tenants().await? {
            let mut tx = self.tenant_tx(tenant).await?;
            let cleared = sqlx::query("UPDATE midas.alerts a SET resolved_at = clock_timestamp() FROM midas.devices d WHERE d.id = a.device_id AND a.kind = $1 AND a.resolved_at IS NULL AND (d.online OR d.enrollment <> 'active')")
                .bind(OFFLINE).execute(&mut *tx).await?.rows_affected();
            resolved += cleared;
            let mut touched = cleared > 0;
            let candidates: Vec<(Uuid, f64)> = sqlx::query_as("SELECT d.id, extract(epoch FROM clock_timestamp() - d.last_seen_at)::float8 FROM midas.devices d WHERE NOT d.online AND d.enrollment = 'active' AND d.last_seen_at IS NOT NULL AND NOT EXISTS (SELECT 1 FROM midas.alerts a WHERE a.device_id = d.id AND a.kind = $1 AND a.resolved_at IS NULL)")
                .bind(OFFLINE).fetch_all(&mut *tx).await?;
            for (device, seconds) in candidates {
                let minutes = crate::policy::effective(&mut tx, tenant, device)
                    .await?
                    .and_then(|settings| {
                        settings
                            .into_iter()
                            .find(|setting| setting.key == policy::OFFLINE_ALERT_MINUTES.key)
                    })
                    .map_or(policy::OFFLINE_ALERT_MINUTES.default, |setting| {
                        setting.value
                    });
                if seconds < (minutes * 60) as f64 {
                    continue;
                }
                let summary = format!("Not seen for {} minutes", (seconds / 60.0).floor());
                if let Raised::New(_) = raise(
                    &mut tx,
                    tenant,
                    device,
                    (OFFLINE, Severity::Critical),
                    &summary,
                )
                .await?
                {
                    opened += 1;
                    touched = true;
                }
            }
            tx.commit().await?;
            if touched {
                self.changed(tenant);
            }
        }
        Ok((opened, resolved))
    }

    /// Sends due notifications; failures retry with backoff and give up after six attempts.
    pub async fn deliver(&self, mailer: &Mailer) -> std::result::Result<usize, sqlx::Error> {
        let Mailer::Smtp { transport, from } = mailer else {
            return Ok(0);
        };
        let mut sent = 0;
        for tenant in self.tenants().await? {
            let mut tx = self.tenant_tx(tenant).await?;
            // Leased for five minutes so a crash during sending does not lose the message.
            let due: Vec<DueRow> = sqlx::query_as("UPDATE midas.alert_notifications n SET next_attempt_at = clock_timestamp() + interval '5 minutes', attempts = n.attempts + 1 FROM (SELECT id FROM midas.alert_notifications WHERE status = 'pending' AND next_attempt_at <= clock_timestamp() ORDER BY next_attempt_at LIMIT $1 FOR UPDATE SKIP LOCKED) due, midas.alerts a, midas.devices d, midas.tenants t WHERE n.id = due.id AND a.id = n.alert_id AND d.id = a.device_id AND t.id = n.tenant_id RETURNING n.id, n.recipient, n.attempts, a.kind, a.severity, a.summary, extract(epoch FROM a.opened_at)::bigint, d.name, t.name")
                .bind(BATCH).fetch_all(&mut *tx).await?;
            tx.commit().await?;
            for row in due {
                let outcome = match message(from, &row.1, &row) {
                    Some(email) => transport.send(email).await.map(|_| ()).map_err(|error| {
                        if error.is_permanent() {
                            ("rejected", true)
                        } else {
                            ("unavailable", false)
                        }
                    }),
                    None => Err(("address", true)),
                };
                let mut tx = self.tenant_tx(tenant).await?;
                match outcome {
                    Ok(()) => {
                        sqlx::query("UPDATE midas.alert_notifications SET status = 'sent', sent_at = clock_timestamp(), last_error = NULL WHERE id = $1")
                            .bind(row.0).execute(&mut *tx).await?;
                        sent += 1;
                    }
                    Err((code, permanent)) => {
                        tracing::warn!(code, attempt = row.2, "alert notification not delivered");
                        let failed = permanent || row.2 >= MAX_ATTEMPTS;
                        sqlx::query("UPDATE midas.alert_notifications SET status = CASE WHEN $2 THEN 'failed' ELSE 'pending' END, next_attempt_at = clock_timestamp() + make_interval(secs => $3), last_error = $4 WHERE id = $1")
                            .bind(row.0).bind(failed).bind(backoff(row.2)).bind(code)
                            .execute(&mut *tx).await?;
                    }
                }
                tx.commit().await?;
            }
        }
        Ok(sent)
    }

    pub async fn alerts(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        open_only: bool,
    ) -> Result<Vec<AlertView>> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let target = Target {
            tenant,
            device: None,
        };
        self.authorizer
            .authorize_view(&identity, target, Permission::MonitorRead)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let alerts = sqlx::query_as("SELECT a.id, a.device_id, d.name AS device_name, a.kind, a.severity, a.summary, extract(epoch FROM a.opened_at)::bigint AS opened_at, extract(epoch FROM a.last_seen_at)::bigint AS last_seen_at, a.occurrences, extract(epoch FROM a.resolved_at)::bigint AS resolved_at, extract(epoch FROM a.acknowledged_at)::bigint AS acknowledged_at, a.acknowledged_by, count(n.id) FILTER (WHERE n.status = 'sent') AS notifications_sent, count(n.id) FILTER (WHERE n.status = 'pending') AS notifications_pending, count(n.id) FILTER (WHERE n.status = 'failed') AS notifications_failed FROM midas.alerts a JOIN midas.devices d ON d.id = a.device_id LEFT JOIN midas.alert_notifications n ON n.alert_id = a.id WHERE NOT $1 OR a.resolved_at IS NULL GROUP BY a.tenant_id, a.id, d.name ORDER BY a.opened_at DESC LIMIT 200")
            .bind(open_only).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(alerts)
    }

    /// Acknowledges once; a second acknowledgement is a conflict.
    pub async fn acknowledge(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        alert: Uuid,
    ) -> Result<()> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let row: Option<(Uuid, String, bool)> = sqlx::query_as(
            "SELECT device_id, kind, acknowledged_at IS NOT NULL FROM midas.alerts WHERE id = $1",
        )
        .bind(alert)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        let (device, kind, acknowledged) = row.ok_or(AccessError::Denied)?;
        let target = Target {
            tenant,
            device: Some(device),
        };
        self.authorizer
            .authorize_identity(&identity, target, Permission::AlertAcknowledge)
            .await?;
        if acknowledged {
            return Err(AccessError::Conflict);
        }
        let mut tx = self.tenant_tx(tenant).await?;
        let updated = sqlx::query("UPDATE midas.alerts SET acknowledged_at = clock_timestamp(), acknowledged_by = $2 WHERE id = $1 AND acknowledged_at IS NULL")
            .bind(alert).bind(identity.operator_id).execute(&mut *tx).await?.rows_affected();
        if updated == 0 {
            return Err(AccessError::Conflict);
        }
        let mut event = Event::new(
            Action::AlertAcknowledge,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("alert_id", alert.to_string())
        .detail("kind", kind);
        event.tenant = Some(tenant);
        event.device = Some(device);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        self.changed(tenant);
        Ok(())
    }

    pub async fn alert_recipients(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Recipients> {
        self.policy_operator(sessions, token, tenant, Permission::PolicyManage, false)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let emails: Option<Vec<String>> =
            sqlx::query_scalar("SELECT emails FROM midas.alert_recipients")
                .fetch_optional(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(Recipients {
            emails: emails.unwrap_or_default(),
        })
    }

    /// Replaces the customer's notification addresses; the audit keeps only their count.
    pub async fn set_alert_recipients(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        recipients: &Recipients,
    ) -> Result<()> {
        let identity = self
            .policy_operator(sessions, token, tenant, Permission::PolicyManage, true)
            .await?;
        let mut emails: Vec<String> = recipients
            .emails
            .iter()
            .map(|email| email.trim().to_owned())
            .collect();
        emails.sort();
        emails.dedup();
        if emails.len() > MAX_RECIPIENTS || !emails.iter().all(|email| valid_email(email)) {
            return Err(AccessError::InvalidInput);
        }
        let mut tx = self.tenant_tx(tenant).await?;
        sqlx::query("INSERT INTO midas.alert_recipients (tenant_id, emails, updated_by) VALUES ($1, $2, $3) ON CONFLICT (tenant_id) DO UPDATE SET emails = EXCLUDED.emails, updated_by = EXCLUDED.updated_by, updated_at = clock_timestamp()")
            .bind(tenant).bind(&emails).bind(identity.operator_id)
            .execute(&mut *tx).await?;
        let mut event = Event::new(
            Action::AlertRecipientsSet,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("recipients", emails.len());
        event.tenant = Some(tenant);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_checked_strictly() {
        for good in ["ops@example.test", "a.b+alerts@sub.example.org"] {
            assert!(valid_email(good), "{good}");
        }
        for bad in [
            "",
            "ops",
            "@example.test",
            "ops@localhost",
            "ops@example.test.",
            "Ops <ops@example.test>",
            "ops@exa mple.test",
            "a,b@example.test",
        ] {
            assert!(!valid_email(bad), "{bad}");
        }
    }

    #[test]
    fn retries_back_off() {
        assert!(backoff(1) < backoff(2) && backoff(4) < backoff(5));
        assert_eq!(backoff(9), backoff(5));
    }

    #[test]
    fn messages_name_the_device_and_refuse_bad_recipients() {
        let from: Mailbox = "MIDAS <midas@example.test>".parse().unwrap();
        let row: DueRow = (
            Uuid::nil(),
            "ops@example.test".into(),
            1,
            OFFLINE.into(),
            "critical".into(),
            "Not seen for 7 minutes".into(),
            1_800_000_000,
            "SRV-01".into(),
            "Contoso".into(),
        );
        let email = message(&from, "ops@example.test", &row).unwrap();
        let text = String::from_utf8(email.formatted()).unwrap();
        assert!(text.contains("Subject: [MIDAS] Critical: SRV-01 - Device offline"));
        assert!(message(&from, "not an address", &row).is_none());
    }
}
