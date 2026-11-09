use crate::database::driver as sqlx;
use midas_protocol::{
    DirectoryAssignment, DirectoryReport,
    directory::{domain_dn, valid_bases},
};
use rustls::pki_types::{CertificateDer, pem::PemObject};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    auth::sessions::{SessionInfo, SessionToken, Sessions},
    authorization::{AccessError, Permission, Target},
    enrollment::{AgentIdentity, Enrollment},
};

/// The tenant's single directory connector: which enrolled device reads which boundary.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorSpec {
    pub device_id: Uuid,
    pub domain: String,
    pub url: String,
    pub ca_pem: String,
    pub base_dns: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ConnectorView {
    pub device_id: Uuid,
    pub domain: String,
    pub url: String,
    pub base_dns: Vec<String>,
    pub revision: i64,
    pub last_check: Option<CheckView>,
}

#[derive(Debug, Serialize)]
pub struct CheckView {
    pub at: i64,
    pub revision: i64,
    pub ok: bool,
    pub objects: i32,
    pub error: Option<String>,
}

type ConnectorRow = (
    Uuid,
    String,
    String,
    Vec<String>,
    i64,
    Option<i64>,
    Option<i64>,
    Option<bool>,
    Option<i32>,
    Option<String>,
);

impl Enrollment {
    pub(crate) async fn directory_operator(
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
                .authorize_identity(&identity, target, Permission::DirectoryDiscover)
                .await?;
        } else {
            self.authorizer
                .authorize_view(&identity, target, Permission::DirectoryDiscover)
                .await?;
        }
        Ok(identity)
    }

    /// Designates the tenant's connector device and its read boundary.
    pub async fn set_connector(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        spec: &ConnectorSpec,
    ) -> Result<i64, AccessError> {
        let identity = self
            .directory_operator(sessions, token, tenant, true)
            .await?;
        let domain = spec.domain.trim().to_ascii_lowercase();
        let url = spec.url.trim();
        let trusted = CertificateDer::pem_slice_iter(spec.ca_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .is_ok_and(|certificates| !certificates.is_empty());
        if domain_dn(&domain).is_none()
            || !url.to_ascii_lowercase().starts_with("ldaps://")
            || url.len() > 255
            || !trusted
            || spec.ca_pem.len() > 16_384
            || !valid_bases(&domain, &spec.base_dns)
        {
            return Err(AccessError::InvalidInput);
        }
        let mut tx = self.tenant_tx(tenant).await?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM midas.devices WHERE id = $1 AND enrollment = 'active')",
        )
        .bind(spec.device_id)
        .fetch_one(&mut *tx)
        .await?;
        if !active {
            return Err(AccessError::InvalidInput);
        }
        let previous: Option<Uuid> = sqlx::query_scalar(
            "SELECT device_id FROM midas.directory_connectors WHERE tenant_id = $1 FOR UPDATE",
        )
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await?;
        let revision: i64 = sqlx::query_scalar("INSERT INTO midas.directory_connectors (tenant_id, device_id, domain, url, ca_pem, base_dns, updated_by) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (tenant_id) DO UPDATE SET device_id = EXCLUDED.device_id, domain = EXCLUDED.domain, url = EXCLUDED.url, ca_pem = EXCLUDED.ca_pem, base_dns = EXCLUDED.base_dns, updated_by = EXCLUDED.updated_by, updated_at = clock_timestamp(), revision = midas.directory_connectors.revision + 1, last_check_at = NULL, last_check_revision = NULL, last_check_ok = NULL, last_check_objects = NULL, last_check_error = NULL RETURNING revision")
            .bind(tenant).bind(spec.device_id).bind(&domain).bind(url).bind(&spec.ca_pem)
            .bind(&spec.base_dns).bind(identity.operator_id)
            .fetch_one(&mut *tx).await?;
        let mut event = Event::new(
            Action::DirectoryConnectorSet,
            Outcome::Success,
            Actor::session(&identity),
        )
        .detail("domain", domain)
        .detail("bases", spec.base_dns.len())
        .detail("revision", revision);
        event.tenant = Some(tenant);
        event.device = Some(spec.device_id);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        for device in previous.into_iter().chain([spec.device_id]) {
            self.signal(device, |signal| signal.refresh += 1);
        }
        Ok(revision)
    }

    pub async fn connector(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<ConnectorView, AccessError> {
        self.directory_operator(sessions, token, tenant, false)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let row: Option<ConnectorRow> = sqlx::query_as("SELECT device_id, domain, url, base_dns, revision, extract(epoch FROM last_check_at)::bigint, last_check_revision, last_check_ok, last_check_objects, last_check_error FROM midas.directory_connectors")
            .fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        let (device_id, domain, url, base_dns, revision, at, checked, ok, objects, error) =
            row.ok_or(AccessError::Denied)?;
        Ok(ConnectorView {
            device_id,
            domain,
            url,
            base_dns,
            revision,
            last_check: match (at, checked, ok) {
                (Some(at), Some(revision), Some(ok)) => Some(CheckView {
                    at,
                    revision,
                    ok,
                    objects: objects.unwrap_or(0),
                    error: error.filter(|e| !e.is_empty()),
                }),
                _ => None,
            },
        })
    }

    /// The assignment for a device that is its tenant's designated connector.
    pub async fn assignment(&self, identity: AgentIdentity) -> Option<DirectoryAssignment> {
        let mut tx = self.tenant_tx(identity.tenant).await.ok()?;
        let row: Option<(i64, String, String, String, Vec<String>)> = sqlx::query_as("SELECT revision, domain, url, ca_pem, base_dns FROM midas.directory_connectors WHERE device_id = $1")
            .bind(identity.device).fetch_optional(&mut *tx).await.ok()?;
        tx.commit().await.ok()?;
        let (revision, domain, url, ca_pem, base_dns) = row?;
        Some(DirectoryAssignment {
            revision: u64::try_from(revision).ok()?,
            domain,
            url,
            ca_pem,
            base_dns,
        })
    }

    /// Stores a check result; reports from other devices or for older revisions are ignored.
    /// A successful scan whose computers all arrived closes it: entries it did not list are
    /// marked removed.
    pub async fn record(&self, identity: AgentIdentity, report: &DirectoryReport) {
        let error = report.error.as_str();
        if error.len() > 32 || !error.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return;
        }
        let Ok(revision) = i64::try_from(report.revision) else {
            return;
        };
        let stored = async {
            let mut tx = self.tenant_tx(identity.tenant).await?;
            let current = sqlx::query("UPDATE midas.directory_connectors SET last_check_at = clock_timestamp(), last_check_revision = revision, last_check_ok = $3, last_check_objects = $4, last_check_error = $5 WHERE device_id = $1 AND revision = $2")
                .bind(identity.device).bind(revision).bind(report.ok)
                .bind(i32::try_from(report.objects).unwrap_or(i32::MAX)).bind(error)
                .execute(&mut *tx).await?.rows_affected() == 1;
            let mut closed = false;
            if current && report.ok {
                let scan = report.scan.cast_signed();
                let arrived: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM midas.directory_computers WHERE scan = $1",
                )
                .bind(scan)
                .fetch_one(&mut *tx)
                .await?;
                if arrived == i64::from(report.objects) {
                    sqlx::query("UPDATE midas.directory_computers SET removed_at = clock_timestamp() WHERE scan <> $1 AND removed_at IS NULL")
                        .bind(scan).execute(&mut *tx).await?;
                    closed = true;
                } else {
                    tracing::warn!(
                        arrived,
                        expected = report.objects,
                        "directory scan incomplete; nothing marked removed"
                    );
                }
            }
            tx.commit().await?;
            if closed {
                self.changed(identity.tenant);
            }
            Ok::<(), sqlx::Error>(())
        };
        if stored.await.is_err() {
            tracing::error!("directory report could not be stored");
        }
    }
}
