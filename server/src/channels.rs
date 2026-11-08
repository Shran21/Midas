//! Teams and Slack alert channels. The webhook URL is the secret: it is stored sealed, never
//! returned, and deliveries go only to the providers' webhook hosts over verified TLS.

use std::{sync::Arc, time::Duration};

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use hmac::{Hmac, Mac};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, body::Bytes, header};
use hyper_util::rt::TokioIo;
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use sqlx::{Postgres, Transaction};
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use uuid::Uuid;
use webauthn_rs::prelude::Url;
use zeroize::Zeroizing;

use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    auth::sessions::{SessionToken, Sessions},
    authorization::{AccessError, Permission},
    enrollment::Enrollment,
};

type Result<T> = std::result::Result<T, AccessError>;

const MAX_CHANNELS: i64 = 10;
const MAX_URL: usize = 1024;
const MAX_ATTEMPTS: i32 = 6;
const BATCH: i64 = 20;
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Teams,
    Slack,
}

impl Kind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Teams => "teams",
            Self::Slack => "slack",
        }
    }

    /// Only the providers' own webhook hosts; anything else could turn the server into a proxy.
    fn allowed(self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        match self {
            Self::Teams => [
                ".webhook.office.com",
                ".logic.azure.com",
                ".environment.api.powerplatform.com",
            ]
            .iter()
            .any(|suffix| host.ends_with(suffix)),
            Self::Slack => host == "hooks.slack.com",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelSpec {
    pub kind: Kind,
    pub label: String,
    pub url: String,
}

/// What operators see of a channel: never its URL.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ChannelView {
    pub id: Uuid,
    pub kind: String,
    pub label: String,
    pub host: String,
    pub created_at: i64,
    pub last_test_at: Option<i64>,
    pub last_test_ok: Option<bool>,
    pub last_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub error: Option<String>,
}

struct Target {
    host: String,
    port: u16,
    path: String,
}

/// Seals channel URLs and posts to them; tests may name a local receiver and its CA.
pub struct Webhooks {
    key: Zeroizing<[u8; 32]>,
    tls: Arc<ClientConfig>,
    test_host: Option<String>,
}

impl Webhooks {
    /// Derives its own key from the server's authentication key, so the two never mix.
    pub fn new(auth_key: &[u8; 32]) -> Self {
        let roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        Self::build(auth_key, roots, None)
    }

    /// Accepts `host` on any port in addition to the providers; for tests with a local receiver.
    pub fn for_tests(auth_key: &[u8; 32], roots: RootCertStore, host: &str) -> Self {
        Self::build(auth_key, roots, Some(host.to_owned()))
    }

    fn build(auth_key: &[u8; 32], roots: RootCertStore, test_host: Option<String>) -> Self {
        let mut derive = <Hmac<Sha256> as hmac::digest::KeyInit>::new_from_slice(auth_key)
            .expect("HMAC takes any key length");
        derive.update(b"midas alert channel urls");
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&derive.finalize().into_bytes());
        let mut tls =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .expect("default TLS versions")
                .with_root_certificates(roots)
                .with_no_client_auth();
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        Self {
            key,
            tls: Arc::new(tls),
            test_host,
        }
    }

    fn aad(tenant: Uuid, channel: Uuid) -> Vec<u8> {
        let mut aad = b"midas-channel:".to_vec();
        aad.extend_from_slice(tenant.as_bytes());
        aad.extend_from_slice(channel.as_bytes());
        aad
    }

    fn seal(&self, tenant: Uuid, channel: Uuid, url: &str) -> Result<Vec<u8>> {
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|_| AccessError::Unavailable)?;
        let aad = Self::aad(tenant, channel);
        let sealed = XChaCha20Poly1305::new((&*self.key).into())
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: url.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| AccessError::Unavailable)?;
        let mut envelope = vec![1];
        envelope.extend(nonce);
        envelope.extend(sealed);
        Ok(envelope)
    }

    fn open(&self, tenant: Uuid, channel: Uuid, envelope: &[u8]) -> Option<Zeroizing<String>> {
        if envelope.len() < 41 || envelope[0] != 1 {
            return None;
        }
        let nonce: [u8; 24] = envelope[1..25].try_into().ok()?;
        let aad = Self::aad(tenant, channel);
        let plain = XChaCha20Poly1305::new((&*self.key).into())
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &envelope[25..],
                    aad: &aad,
                },
            )
            .ok()?;
        String::from_utf8(plain).ok().map(Zeroizing::new)
    }

    fn target(&self, kind: Kind, text: &str) -> Option<Target> {
        if text.len() > MAX_URL {
            return None;
        }
        let url = Url::parse(text).ok()?;
        let host = url.host_str()?.to_ascii_lowercase();
        let test = self.test_host.as_deref() == Some(host.as_str());
        let ok = url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none()
            && (test || (kind.allowed(&host) && url.port().is_none_or(|port| port == 443)));
        let path = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_owned(),
        };
        ok.then(|| Target {
            port: url.port().unwrap_or(443),
            host,
            path,
        })
    }

    /// Posts JSON; the result is the HTTP status, or a short error code.
    async fn post(
        &self,
        kind: Kind,
        url: &str,
        text: &str,
    ) -> std::result::Result<u16, &'static str> {
        let target = self.target(kind, url).ok_or("invalid_url")?;
        let body = serde_json::json!({ "text": text }).to_string();
        let name = ServerName::try_from(target.host.clone()).map_err(|_| "invalid_url")?;
        let exchange = async {
            let tcp = tokio::net::TcpStream::connect((target.host.as_str(), target.port))
                .await
                .map_err(|_| "connect")?;
            let tls = TlsConnector::from(self.tls.clone())
                .connect(name, tcp)
                .await
                .map_err(|_| "tls")?;
            let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
                .await
                .map_err(|_| "http")?;
            let driver = tokio::spawn(connection);
            let request = Request::post(target.path.as_str())
                .header(header::HOST, target.host.as_str())
                .header(header::CONTENT_TYPE, "application/json")
                .body(Full::new(Bytes::from(body)))
                .map_err(|_| "http")?;
            let response = sender.send_request(request).await.map_err(|_| "http");
            let status = match response {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let _ = Limited::new(response.into_body(), 16 * 1024)
                        .collect()
                        .await;
                    Ok(status)
                }
                Err(error) => Err(error),
            };
            driver.abort();
            status
        };
        timeout(TIMEOUT, exchange).await.map_err(|_| "timeout")?
    }
}

fn status_error(status: u16) -> (&'static str, bool) {
    match status {
        200..=299 => ("", false),
        408 | 429 => ("throttled", false),
        400..=499 => ("refused", true),
        _ => ("server_error", false),
    }
}

impl Enrollment {
    fn webhooks(&self) -> Result<&Webhooks> {
        self.webhooks
            .get()
            .map(Arc::as_ref)
            .ok_or(AccessError::Unavailable)
    }

    pub async fn channels(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Vec<ChannelView>> {
        self.policy_operator(sessions, token, tenant, Permission::PolicyManage, false)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let channels = sqlx::query_as("SELECT id, kind, label, host, extract(epoch FROM created_at)::bigint AS created_at, extract(epoch FROM last_test_at)::bigint AS last_test_at, last_test_ok, last_error FROM midas.alert_channels ORDER BY created_at, id")
            .fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(channels)
    }

    pub async fn add_channel(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        spec: &ChannelSpec,
    ) -> Result<ChannelView> {
        let identity = self
            .policy_operator(sessions, token, tenant, Permission::PolicyManage, true)
            .await?;
        let webhooks = self.webhooks()?;
        let label = spec.label.trim();
        if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
            return Err(AccessError::InvalidInput);
        }
        let target = webhooks
            .target(spec.kind, spec.url.trim())
            .ok_or(AccessError::InvalidInput)?;
        let id = Uuid::new_v4();
        let sealed = webhooks.seal(tenant, id, spec.url.trim())?;
        let mut tx = self.tenant_tx(tenant).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.alert_channels")
            .fetch_one(&mut *tx)
            .await?;
        if count >= MAX_CHANNELS {
            return Err(AccessError::Conflict);
        }
        let view: ChannelView = sqlx::query_as("INSERT INTO midas.alert_channels (tenant_id, id, kind, label, host, sealed, created_by) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id, kind, label, host, extract(epoch FROM created_at)::bigint AS created_at, NULL::bigint AS last_test_at, last_test_ok, last_error")
            .bind(tenant).bind(id).bind(spec.kind.as_str()).bind(label).bind(&target.host)
            .bind(&sealed).bind(identity.operator_id).fetch_one(&mut *tx).await?;
        record(
            &mut tx,
            Action::AlertChannelAdd,
            &identity,
            tenant,
            &view,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(view)
    }

    pub async fn remove_channel(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        channel: Uuid,
    ) -> Result<()> {
        let identity = self
            .policy_operator(sessions, token, tenant, Permission::PolicyManage, true)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        sqlx::query("DELETE FROM midas.alert_deliveries WHERE channel_id = $1")
            .bind(channel)
            .execute(&mut *tx)
            .await?;
        let removed: Option<ChannelView> = sqlx::query_as("DELETE FROM midas.alert_channels WHERE id = $1 RETURNING id, kind, label, host, extract(epoch FROM created_at)::bigint AS created_at, extract(epoch FROM last_test_at)::bigint AS last_test_at, last_test_ok, last_error")
            .bind(channel).fetch_optional(&mut *tx).await?;
        let removed = removed.ok_or(AccessError::Denied)?;
        record(
            &mut tx,
            Action::AlertChannelRemove,
            &identity,
            tenant,
            &removed,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Sends a clearly marked test message at once and records the outcome.
    pub async fn test_channel(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        channel: Uuid,
    ) -> Result<TestResult> {
        let identity = self
            .policy_operator(sessions, token, tenant, Permission::PolicyManage, true)
            .await?;
        let webhooks = self.webhooks()?;
        let mut tx = self.tenant_tx(tenant).await?;
        let row: Option<(String, Vec<u8>, String)> = sqlx::query_as(
            "SELECT kind, sealed, (SELECT name FROM midas.tenants WHERE id = $2) FROM midas.alert_channels WHERE id = $1",
        )
        .bind(channel)
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        let (kind, sealed, tenant_name) = row.ok_or(AccessError::Denied)?;
        let kind = if kind == "teams" {
            Kind::Teams
        } else {
            Kind::Slack
        };
        let url = webhooks
            .open(tenant, channel, &sealed)
            .ok_or(AccessError::Unavailable)?;
        let text = format!(
            "[MIDAS] Próbariasztás / Test alert\nÜgyfél / Customer: {tenant_name}\nEz a csatorna megkapja a MIDAS riasztásait. / This channel receives MIDAS alerts."
        );
        let error = match webhooks.post(kind, &url, &text).await {
            Ok(status) => {
                let (code, _) = status_error(status);
                (!code.is_empty()).then_some(code)
            }
            Err(code) => Some(code),
        };
        let mut tx = self.tenant_tx(tenant).await?;
        let view: ChannelView = sqlx::query_as("UPDATE midas.alert_channels SET last_test_at = clock_timestamp(), last_test_ok = $2, last_error = $3 WHERE id = $1 RETURNING id, kind, label, host, extract(epoch FROM created_at)::bigint AS created_at, extract(epoch FROM last_test_at)::bigint AS last_test_at, last_test_ok, last_error")
            .bind(channel).bind(error.is_none()).bind(error).fetch_one(&mut *tx).await?;
        record(
            &mut tx,
            Action::AlertChannelTest,
            &identity,
            tenant,
            &view,
            error,
        )
        .await?;
        tx.commit().await?;
        Ok(TestResult {
            ok: error.is_none(),
            error: error.map(str::to_owned),
        })
    }

    /// Posts due alert messages to the channels; transient failures retry with backoff.
    pub async fn deliver_webhooks(&self) -> std::result::Result<usize, sqlx::Error> {
        let Some(webhooks) = self.webhooks.get() else {
            return Ok(0);
        };
        let mut sent = 0;
        for tenant in self.tenants().await? {
            let mut tx = self.tenant_tx(tenant).await?;
            let due: Vec<DueRow> = sqlx::query_as("UPDATE midas.alert_deliveries d SET next_attempt_at = clock_timestamp() + interval '5 minutes', attempts = d.attempts + 1 FROM (SELECT id FROM midas.alert_deliveries WHERE status = 'pending' AND next_attempt_at <= clock_timestamp() ORDER BY next_attempt_at LIMIT $1 FOR UPDATE SKIP LOCKED) due, midas.alert_channels c, midas.alerts a, midas.devices v, midas.tenants t WHERE d.id = due.id AND c.id = d.channel_id AND a.id = d.alert_id AND v.id = a.device_id AND t.id = d.tenant_id RETURNING d.id, d.attempts, c.id, c.kind, c.sealed, a.kind, a.severity, a.summary, extract(epoch FROM a.opened_at)::bigint, v.name, t.name")
                .bind(BATCH).fetch_all(&mut *tx).await?;
            tx.commit().await?;
            for row in due {
                let (
                    delivery,
                    attempts,
                    channel,
                    kind,
                    sealed,
                    alert,
                    severity,
                    summary,
                    opened,
                    device,
                    tenant_name,
                ) = row;
                let kind = if kind == "teams" {
                    Kind::Teams
                } else {
                    Kind::Slack
                };
                let text =
                    crate::alerts::text(&alert, &severity, &summary, opened, &device, &tenant_name)
                        .map(|(subject, body)| format!("{subject}\n{body}"));
                let outcome = match (webhooks.open(tenant, channel, &sealed), text) {
                    (Some(url), Some(text)) => match webhooks.post(kind, &url, &text).await {
                        Ok(status) => match status_error(status) {
                            ("", _) => Ok(()),
                            (code, permanent) => Err((code, permanent)),
                        },
                        Err(code) => Err((code, false)),
                    },
                    _ => Err(("sealed", true)),
                };
                let mut tx = self.tenant_tx(tenant).await?;
                match outcome {
                    Ok(()) => {
                        sqlx::query("UPDATE midas.alert_deliveries SET status = 'sent', last_error = NULL WHERE id = $1")
                            .bind(delivery).execute(&mut *tx).await?;
                        sent += 1;
                    }
                    Err((code, permanent)) => {
                        tracing::warn!(code, attempt = attempts, "alert webhook not delivered");
                        let failed = permanent || attempts >= MAX_ATTEMPTS;
                        sqlx::query("UPDATE midas.alert_deliveries SET status = CASE WHEN $2 THEN 'failed' ELSE 'pending' END, next_attempt_at = clock_timestamp() + make_interval(secs => $3), last_error = $4 WHERE id = $1")
                            .bind(delivery).bind(failed).bind(crate::alerts::backoff(attempts)).bind(code)
                            .execute(&mut *tx).await?;
                    }
                }
                tx.commit().await?;
            }
        }
        Ok(sent)
    }
}

type DueRow = (
    Uuid,
    i32,
    Uuid,
    String,
    Vec<u8>,
    String,
    String,
    String,
    i64,
    String,
    String,
);

async fn record(
    tx: &mut Transaction<'_, Postgres>,
    action: Action,
    identity: &crate::auth::sessions::SessionInfo,
    tenant: Uuid,
    channel: &ChannelView,
    error: Option<&str>,
) -> std::result::Result<(), sqlx::Error> {
    let mut event = Event::new(action, Outcome::Success, Actor::session(identity))
        .detail("channel_id", channel.id.to_string())
        .detail("kind", channel.kind.clone())
        .detail("host", channel.host.clone());
    if matches!(action, Action::AlertChannelTest) {
        event = event.detail("result", error.unwrap_or("ok"));
    }
    event.tenant = Some(tenant);
    audit::append_tenant(tx, &event).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_provider_hosts_over_https_are_accepted_and_urls_stay_sealed() {
        let hooks = Webhooks::new(&[7; 32]);
        for (kind, url) in [
            (Kind::Slack, "https://hooks.slack.com/services/T0/B0/secret"),
            (
                Kind::Teams,
                "https://contoso.webhook.office.com/webhookb2/abc/IncomingWebhook/x/y",
            ),
            (
                Kind::Teams,
                "https://prod-01.westeurope.logic.azure.com:443/workflows/a/triggers/manual/paths/invoke?sig=s",
            ),
        ] {
            assert!(hooks.target(kind, url).is_some(), "{url}");
        }
        for (kind, url) in [
            (Kind::Slack, "http://hooks.slack.com/services/x"),
            (Kind::Slack, "https://hooks.slack.com.evil.example/x"),
            (Kind::Slack, "https://user:pw@hooks.slack.com/x"),
            (Kind::Slack, "https://hooks.slack.com:8443/x"),
            (Kind::Teams, "https://hooks.slack.com/services/x"),
            (Kind::Slack, "https://127.0.0.1/x"),
            (Kind::Teams, "https://webhook.office.com.attacker.test/x"),
        ] {
            assert!(hooks.target(kind, url).is_none(), "{url}");
        }
        let (tenant, channel) = (Uuid::new_v4(), Uuid::new_v4());
        let url = "https://hooks.slack.com/services/T0/B0/secret-part";
        let sealed = hooks.seal(tenant, channel, url).unwrap();
        assert!(!sealed.windows(11).any(|w| w == b"secret-part"));
        assert_eq!(hooks.open(tenant, channel, &sealed).unwrap().as_str(), url);
        assert!(
            hooks.open(Uuid::new_v4(), channel, &sealed).is_none(),
            "bound to its tenant"
        );
        assert!(
            Webhooks::new(&[8; 32])
                .open(tenant, channel, &sealed)
                .is_none()
        );
        assert_eq!(status_error(204), ("", false));
        assert_eq!(status_error(404), ("refused", true));
        assert_eq!(status_error(429), ("throttled", false));
    }
}
