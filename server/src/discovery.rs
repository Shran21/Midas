use std::collections::HashMap;

use midas_protocol::{
    DirectoryChunk, MAX_DIRECTORY_CHUNK, MAX_DIRECTORY_DN, MAX_DIRECTORY_TEXT, directory::within,
};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    auth::sessions::{SessionToken, Sessions},
    authorization::AccessError,
    devices::Named,
    enrollment::{AgentIdentity, Enrollment},
};

const MAX_LISTED: i64 = 20_000;

/// A discovered computer and how it relates to the enrolled devices of the same tenant.
#[derive(Debug, Serialize)]
pub struct ComputerView {
    pub domain: String,
    pub object_guid: Uuid,
    pub name: String,
    pub dns_host_name: String,
    pub distinguished_name: String,
    pub operating_system: String,
    pub operating_system_version: String,
    pub last_logon_at: Option<i64>,
    pub disabled: bool,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub removed_at: Option<i64>,
    /// `candidate` (no agent), `managed` (one agent proved this identity) or `conflict` (several did).
    pub status: &'static str,
    /// Active devices whose agent reported this objectGUID and domain.
    pub devices: Vec<Named>,
    /// Devices with the same host name but no proven identity; a hint, never a match.
    pub name_matches: Vec<Named>,
}

#[derive(sqlx::FromRow)]
struct Row {
    domain: String,
    object_guid: Uuid,
    name: String,
    dns_host_name: String,
    distinguished_name: String,
    operating_system: String,
    operating_system_version: String,
    last_logon_at: Option<i64>,
    disabled: bool,
    first_seen_at: i64,
    last_seen_at: i64,
    removed_at: Option<i64>,
}

fn text(value: &str, limit: usize) -> bool {
    value.chars().count() <= limit && !value.contains('\0')
}

#[derive(Default)]
struct Columns {
    guid: Vec<Uuid>,
    name: Vec<String>,
    host: Vec<String>,
    dn: Vec<String>,
    os: Vec<String>,
    version: Vec<String>,
    logon: Vec<i64>,
    disabled: Vec<bool>,
}

impl Enrollment {
    /// Stores part of a scan from the tenant's designated connector. The server checks the
    /// boundary again, so an entry outside it is dropped even if the connector sent it.
    pub async fn store_directory(
        &self,
        identity: AgentIdentity,
        chunk: &DirectoryChunk,
    ) -> Result<(), sqlx::Error> {
        if chunk.computers.len() > MAX_DIRECTORY_CHUNK || chunk.scan == 0 {
            return Err(sqlx::Error::Protocol("invalid directory chunk".into()));
        }
        let Ok(revision) = i64::try_from(chunk.revision) else {
            return Ok(());
        };
        let mut tx = self.tenant_tx(identity.tenant).await?;
        let connector: Option<(String, Vec<String>)> = sqlx::query_as("SELECT lower(domain), base_dns FROM midas.directory_connectors WHERE device_id = $1 AND revision = $2")
            .bind(identity.device).bind(revision).fetch_optional(&mut *tx).await?;
        let Some((domain, bases)) = connector else {
            return Ok(());
        };
        let mut columns = Columns::default();
        for computer in &chunk.computers {
            let Ok(guid) = <[u8; 16]>::try_from(computer.object_guid.as_slice()) else {
                continue;
            };
            let valid = !computer.distinguished_name.is_empty()
                && text(&computer.distinguished_name, MAX_DIRECTORY_DN)
                && [
                    &computer.name,
                    &computer.dns_host_name,
                    &computer.operating_system,
                    &computer.operating_system_version,
                ]
                .iter()
                .all(|value| text(value, MAX_DIRECTORY_TEXT))
                && bases
                    .iter()
                    .any(|base| within(&computer.distinguished_name, base));
            if !valid {
                continue;
            }
            columns.guid.push(Uuid::from_bytes_le(guid));
            columns.name.push(computer.name.clone());
            columns.host.push(computer.dns_host_name.to_lowercase());
            columns.dn.push(computer.distinguished_name.clone());
            columns.os.push(computer.operating_system.clone());
            columns
                .version
                .push(computer.operating_system_version.clone());
            columns.logon.push(computer.last_logon.max(0));
            columns.disabled.push(computer.disabled);
        }
        let skipped = chunk.computers.len() - columns.guid.len();
        if skipped > 0 {
            tracing::warn!(skipped, "invalid directory entries skipped");
        }
        sqlx::query("INSERT INTO midas.directory_computers (tenant_id, domain, object_guid, name, dns_host_name, distinguished_name, operating_system, operating_system_version, last_logon_at, disabled, scan) SELECT $1, $2, g, n, h, dn, os, v, CASE WHEN l > 0 THEN to_timestamp(l) END, d, $3 FROM unnest($4::uuid[], $5::text[], $6::text[], $7::text[], $8::text[], $9::text[], $10::bigint[], $11::bool[]) AS u(g, n, h, dn, os, v, l, d) ON CONFLICT (tenant_id, domain, object_guid) DO UPDATE SET name = EXCLUDED.name, dns_host_name = EXCLUDED.dns_host_name, distinguished_name = EXCLUDED.distinguished_name, operating_system = EXCLUDED.operating_system, operating_system_version = EXCLUDED.operating_system_version, last_logon_at = EXCLUDED.last_logon_at, disabled = EXCLUDED.disabled, scan = EXCLUDED.scan, last_seen_at = clock_timestamp(), removed_at = NULL")
            .bind(identity.tenant).bind(&domain).bind(chunk.scan.cast_signed())
            .bind(&columns.guid).bind(&columns.name).bind(&columns.host).bind(&columns.dn)
            .bind(&columns.os).bind(&columns.version).bind(&columns.logon).bind(&columns.disabled)
            .execute(&mut *tx).await?;
        tx.commit().await
    }

    /// Discovered computers with their verified device association, newest first.
    pub async fn directory_computers(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Vec<ComputerView>, AccessError> {
        self.directory_operator(sessions, token, tenant, false)
            .await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let rows: Vec<Row> = sqlx::query_as("SELECT domain, object_guid, name, dns_host_name, distinguished_name, operating_system, operating_system_version, extract(epoch FROM last_logon_at)::bigint AS last_logon_at, disabled, extract(epoch FROM first_seen_at)::bigint AS first_seen_at, extract(epoch FROM last_seen_at)::bigint AS last_seen_at, extract(epoch FROM removed_at)::bigint AS removed_at FROM midas.directory_computers WHERE removed_at IS NULL OR removed_at > clock_timestamp() - interval '30 days' ORDER BY removed_at IS NOT NULL, lower(name), object_guid LIMIT $1")
            .bind(MAX_LISTED).fetch_all(&mut *tx).await?;
        // Only an identity the device's own agent reported counts; the tenant scope comes from RLS.
        let claims: Vec<(Uuid, String, String, String)> = sqlx::query_as("SELECT d.id, d.name, lower(g.value), lower(m.value) FROM midas.devices d JOIN midas.device_inventory g ON g.tenant_id = d.tenant_id AND g.device_id = d.id AND g.kind = 'directory' AND g.key = 'object_guid' JOIN midas.device_inventory m ON m.tenant_id = d.tenant_id AND m.device_id = d.id AND m.kind = 'directory' AND m.key = 'domain' WHERE d.enrollment = 'active'")
            .fetch_all(&mut *tx).await?;
        let devices: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT id, name FROM midas.devices WHERE enrollment = 'active'")
                .fetch_all(&mut *tx)
                .await?;
        tx.commit().await?;
        let mut proven: HashMap<(String, Uuid), Vec<Named>> = HashMap::new();
        let mut claiming = std::collections::HashSet::new();
        for (id, name, guid, domain) in claims {
            claiming.insert(id);
            if let Ok(guid) = Uuid::parse_str(guid.trim_matches(['{', '}'])) {
                proven
                    .entry((domain, guid))
                    .or_default()
                    .push(Named { id, name });
            }
        }
        let mut by_name: HashMap<String, Vec<Named>> = HashMap::new();
        for (id, name) in devices {
            if !claiming.contains(&id) {
                by_name
                    .entry(name.to_lowercase())
                    .or_default()
                    .push(Named { id, name });
            }
        }
        Ok(rows
            .into_iter()
            .map(|row| {
                let devices = proven
                    .remove(&(row.domain.clone(), row.object_guid))
                    .unwrap_or_default();
                let host = row.dns_host_name.split('.').next().unwrap_or_default();
                let mut name_matches = Vec::new();
                for key in [row.name.to_lowercase(), host.to_owned()] {
                    for device in by_name.get(&key).into_iter().flatten() {
                        if !name_matches
                            .iter()
                            .any(|known: &Named| known.id == device.id)
                        {
                            name_matches.push(Named {
                                id: device.id,
                                name: device.name.clone(),
                            });
                        }
                    }
                }
                ComputerView {
                    status: match devices.len() {
                        0 => "candidate",
                        1 => "managed",
                        _ => "conflict",
                    },
                    devices,
                    name_matches,
                    domain: row.domain,
                    object_guid: row.object_guid,
                    name: row.name,
                    dns_host_name: row.dns_host_name,
                    distinguished_name: row.distinguished_name,
                    operating_system: row.operating_system,
                    operating_system_version: row.operating_system_version,
                    last_logon_at: row.last_logon_at,
                    disabled: row.disabled,
                    first_seen_at: row.first_seen_at,
                    last_seen_at: row.last_seen_at,
                    removed_at: row.removed_at,
                }
            })
            .collect())
    }
}
