use crate::database::driver as sqlx;
use std::{collections::HashMap, time::Duration, time::Instant};

use serde::Serialize;
use uuid::Uuid;

use crate::{
    alerts::AlertView,
    audit::{self, Action, Actor, Event, Outcome},
    auth::sessions::{SessionToken, Sessions},
    authorization::{AccessError, Permission, Target},
    enrollment::Enrollment,
};

type Result<T> = std::result::Result<T, AccessError>;

/// A repeated request within this window is accepted without waking the agent again.
const REFRESH_WINDOW: Duration = Duration::from_secs(30);
const MAX_DEVICES: i64 = 5000;

#[derive(Debug, Serialize)]
pub struct Named {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct LatestMetrics {
    pub at: i64,
    pub cpu_permille: i16,
    pub memory_used: i64,
    pub memory_total: i64,
    pub disk_used: i64,
    pub disk_total: i64,
}

#[derive(Debug, Serialize)]
pub struct DeviceSummary {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub platform: String,
    pub enrollment: String,
    pub site: Option<Named>,
    pub groups: Vec<Named>,
    pub online: bool,
    pub last_seen_at: Option<i64>,
    pub decided_at: Option<i64>,
    pub os: Option<String>,
    pub capabilities: Vec<String>,
    /// Worst reported check state; `None` before the first report.
    pub health: Option<String>,
    pub health_reported_at: Option<i64>,
    pub inventory_reconciled_at: Option<i64>,
    pub restart_pending: bool,
    pub open_alerts: i64,
    pub critical_alerts: i64,
    pub metrics: Option<LatestMetrics>,
    /// The operator's effective permissions for this device, as the server checks them.
    pub permissions: Vec<Permission>,
}

struct Row {
    id: Uuid,
    name: String,
    kind: String,
    platform: String,
    enrollment: String,
    site_id: Option<Uuid>,
    site_name: Option<String>,
    online: bool,
    last_seen_at: Option<i64>,
    decided_at: Option<i64>,
    os: Option<String>,
    capabilities: Vec<String>,
    health: Option<String>,
    health_reported_at: Option<i64>,
    inventory_reconciled_at: Option<i64>,
    restart_pending: bool,
    open_alerts: i64,
    critical_alerts: i64,
    metrics_at: Option<i64>,
    cpu_permille: Option<i16>,
    memory_used: Option<i64>,
    memory_total: Option<i64>,
    disk_used: Option<i64>,
    disk_total: Option<i64>,
}
crate::database::postgres_row!(Row {
    id,
    name,
    kind,
    platform,
    enrollment,
    site_id,
    site_name,
    online,
    last_seen_at,
    decided_at,
    os,
    capabilities,
    health,
    health_reported_at,
    inventory_reconciled_at,
    restart_pending,
    open_alerts,
    critical_alerts,
    metrics_at,
    cpu_permille,
    memory_used,
    memory_total,
    disk_used,
    disk_total
});

#[derive(Debug, Serialize)]
pub struct Sample {
    pub at: i64,
    pub cpu_permille: i16,
    pub memory_used: i64,
    pub memory_total: i64,
    pub disk_used: i64,
    pub disk_total: i64,
    pub net_rx: i64,
    pub net_tx: i64,
}
crate::database::postgres_row!(Sample {
    at,
    cpu_permille,
    memory_used,
    memory_total,
    disk_used,
    disk_total,
    net_rx,
    net_tx
});

#[derive(Debug, Serialize)]
pub struct HourlySample {
    pub hour: i64,
    pub cpu_avg_permille: i16,
    pub cpu_max_permille: i16,
    pub memory_used_avg: i64,
    pub memory_total: i64,
    pub disk_used_max: i64,
    pub disk_total: i64,
}
crate::database::postgres_row!(HourlySample {
    hour,
    cpu_avg_permille,
    cpu_max_permille,
    memory_used_avg,
    memory_total,
    disk_used_max,
    disk_total
});

#[derive(Debug, Serialize)]
pub struct MetricsView {
    /// The last hour of one-minute samples.
    pub recent: Vec<Sample>,
    /// The last 24 completed hours.
    pub hourly: Vec<HourlySample>,
}

impl Enrollment {
    /// Active and revoked devices the operator may read, with what they may do on each.
    pub async fn devices(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
    ) -> Result<Vec<DeviceSummary>> {
        self.summaries(sessions, token, tenant, None).await
    }

    pub async fn device(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<DeviceSummary> {
        self.summaries(sessions, token, tenant, Some(device))
            .await?
            .pop()
            .ok_or(AccessError::Denied)
    }

    async fn summaries(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Option<Uuid>,
    ) -> Result<Vec<DeviceSummary>> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let mut permissions = self
            .authorizer
            .device_permissions(&identity, tenant, device)
            .await?;
        permissions.retain(|_, held| held.contains(&Permission::DeviceRead));
        if permissions.is_empty() {
            return Ok(Vec::new());
        }
        let readable: Vec<Uuid> = permissions.keys().copied().collect();
        let mut tx = self.tenant_tx(tenant).await?;
        let rows: Vec<Row> = sqlx::query_as("SELECT d.id, d.name, d.kind, d.platform, d.enrollment, d.site_id, s.name AS site_name, d.online, extract(epoch FROM d.last_seen_at)::bigint AS last_seen_at, extract(epoch FROM d.decided_at)::bigint AS decided_at, (SELECT i.value FROM midas.device_inventory i WHERE i.tenant_id = d.tenant_id AND i.device_id = d.id AND i.kind = 'os' AND i.key IN ('long_version', 'name') ORDER BY i.key = 'long_version' DESC LIMIT 1) AS os, d.capabilities, (SELECT CASE WHEN bool_or(h.state = 'critical') THEN 'critical' WHEN bool_or(h.state = 'warning') THEN 'warning' WHEN bool_or(h.state = 'ok') THEN 'ok' ELSE 'unavailable' END FROM midas.device_health h WHERE h.tenant_id = d.tenant_id AND h.device_id = d.id) AS health, extract(epoch FROM d.health_reported_at)::bigint AS health_reported_at, extract(epoch FROM d.inventory_reconciled_at)::bigint AS inventory_reconciled_at, EXISTS (SELECT 1 FROM midas.device_health h WHERE h.tenant_id = d.tenant_id AND h.device_id = d.id AND h.check_id = 'updates' AND h.code = 'reboot_pending') AS restart_pending, (SELECT count(*) FROM midas.alerts a WHERE a.tenant_id = d.tenant_id AND a.device_id = d.id AND a.resolved_at IS NULL) AS open_alerts, (SELECT count(*) FROM midas.alerts a WHERE a.tenant_id = d.tenant_id AND a.device_id = d.id AND a.resolved_at IS NULL AND a.severity = 'critical') AS critical_alerts, m.at AS metrics_at, m.cpu_permille, m.memory_used, m.memory_total, m.disk_used, m.disk_total FROM midas.devices d LEFT JOIN midas.sites s ON s.tenant_id = d.tenant_id AND s.id = d.site_id LEFT JOIN LATERAL (SELECT extract(epoch FROM x.at)::bigint AS at, x.cpu_permille, x.memory_used, x.memory_total, x.disk_used, x.disk_total FROM midas.device_metrics x WHERE x.tenant_id = d.tenant_id AND x.device_id = d.id ORDER BY x.at DESC LIMIT 1) m ON true WHERE d.id = ANY($1) AND d.enrollment IN ('active', 'revoked') ORDER BY lower(d.name), d.id LIMIT $2")
            .bind(&readable).bind(MAX_DEVICES).fetch_all(&mut *tx).await?;
        let memberships: Vec<(Uuid, Uuid, String)> = sqlx::query_as("SELECT m.device_id, g.id, g.name FROM midas.device_group_members m JOIN midas.device_groups g ON g.tenant_id = m.tenant_id AND g.id = m.group_id WHERE m.device_id = ANY($1) ORDER BY lower(g.name), g.id")
            .bind(&readable).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        let mut groups: HashMap<Uuid, Vec<Named>> = HashMap::new();
        for (device, id, name) in memberships {
            groups.entry(device).or_default().push(Named { id, name });
        }
        Ok(rows
            .into_iter()
            .map(|row| summary(row, &mut groups, &mut permissions))
            .collect())
    }

    /// One-minute samples of the last hour and hourly rollups of the last day.
    pub async fn device_metrics(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<MetricsView> {
        self.monitor(sessions, token, tenant, device).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let mut recent: Vec<Sample> = sqlx::query_as("SELECT extract(epoch FROM at)::bigint AS at, cpu_permille, memory_used, memory_total, disk_used, disk_total, net_rx, net_tx FROM midas.device_metrics WHERE device_id = $1 AND at > clock_timestamp() - interval '1 hour' ORDER BY at DESC LIMIT 60")
            .bind(device).fetch_all(&mut *tx).await?;
        let mut hourly: Vec<HourlySample> = sqlx::query_as("SELECT extract(epoch FROM hour)::bigint AS hour, cpu_avg_permille, cpu_max_permille, memory_used_avg, memory_total, disk_used_max, disk_total FROM midas.device_metrics_hourly WHERE device_id = $1 AND hour > clock_timestamp() - interval '25 hours' ORDER BY hour DESC LIMIT 24")
            .bind(device).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        recent.reverse();
        hourly.reverse();
        Ok(MetricsView { recent, hourly })
    }

    /// The device's latest alerts, open ones first.
    pub async fn device_alerts(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<Vec<AlertView>> {
        self.monitor(sessions, token, tenant, device).await?;
        let mut tx = self.tenant_tx(tenant).await?;
        let alerts = sqlx::query_as("SELECT a.id, a.device_id, d.name AS device_name, a.kind, a.severity, a.summary, extract(epoch FROM a.opened_at)::bigint AS opened_at, extract(epoch FROM a.last_seen_at)::bigint AS last_seen_at, a.occurrences, extract(epoch FROM a.resolved_at)::bigint AS resolved_at, extract(epoch FROM a.acknowledged_at)::bigint AS acknowledged_at, a.acknowledged_by, count(n.id) FILTER (WHERE n.status = 'sent') AS notifications_sent, count(n.id) FILTER (WHERE n.status = 'pending') AS notifications_pending, count(n.id) FILTER (WHERE n.status = 'failed') AS notifications_failed FROM midas.alerts a JOIN midas.devices d ON d.id = a.device_id LEFT JOIN midas.alert_notifications n ON n.alert_id = a.id WHERE a.device_id = $1 GROUP BY a.tenant_id, a.id, d.name ORDER BY a.resolved_at IS NULL DESC, a.opened_at DESC LIMIT 50")
            .bind(device).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(alerts)
    }

    /// Asks the connected agent for fresh health checks and inventory; an offline device cannot answer.
    pub async fn refresh(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<()> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let target = Target {
            tenant,
            device: Some(device),
        };
        self.authorizer
            .authorize_identity(&identity, target, Permission::MonitorRead)
            .await?;
        let now = Instant::now();
        {
            let mut refreshed = self.refreshed.lock().unwrap_or_else(|e| e.into_inner());
            refreshed.retain(|_, at| now.duration_since(*at) < REFRESH_WINDOW);
            if refreshed.contains_key(&device) {
                return Ok(());
            }
            if !self.signal(device, |signal| signal.checks += 1) {
                return Err(AccessError::Offline);
            }
            refreshed.insert(device, now);
        }
        let mut tx = self.tenant_tx(tenant).await?;
        let mut event = Event::new(
            Action::DeviceRefresh,
            Outcome::Success,
            Actor::session(&identity),
        );
        event.tenant = Some(tenant);
        event.device = Some(device);
        audit::append_tenant(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn monitor(
        &self,
        sessions: &Sessions,
        token: &SessionToken,
        tenant: Uuid,
        device: Uuid,
    ) -> Result<()> {
        if tenant.is_nil() {
            return Err(AccessError::InvalidInput);
        }
        let identity = sessions.validate(token, false).await?;
        let target = Target {
            tenant,
            device: Some(device),
        };
        self.authorizer
            .authorize_view(&identity, target, Permission::MonitorRead)
            .await?;
        Ok(())
    }
}

fn summary(
    row: Row,
    groups: &mut HashMap<Uuid, Vec<Named>>,
    permissions: &mut HashMap<Uuid, Vec<Permission>>,
) -> DeviceSummary {
    let metrics = match (
        row.metrics_at,
        row.cpu_permille,
        row.memory_used,
        row.memory_total,
        row.disk_used,
        row.disk_total,
    ) {
        (
            Some(at),
            Some(cpu),
            Some(memory_used),
            Some(memory_total),
            Some(disk_used),
            Some(disk_total),
        ) => Some(LatestMetrics {
            at,
            cpu_permille: cpu,
            memory_used,
            memory_total,
            disk_used,
            disk_total,
        }),
        _ => None,
    };
    DeviceSummary {
        id: row.id,
        name: row.name,
        kind: row.kind,
        platform: row.platform,
        enrollment: row.enrollment,
        site: row
            .site_id
            .zip(row.site_name)
            .map(|(id, name)| Named { id, name }),
        groups: groups.remove(&row.id).unwrap_or_default(),
        online: row.online,
        last_seen_at: row.last_seen_at,
        decided_at: row.decided_at,
        os: row.os,
        capabilities: row.capabilities,
        health: row.health,
        health_reported_at: row.health_reported_at,
        inventory_reconciled_at: row.inventory_reconciled_at,
        restart_pending: row.restart_pending,
        open_alerts: row.open_alerts,
        critical_alerts: row.critical_alerts,
        metrics,
        permissions: permissions.remove(&row.id).unwrap_or_default(),
    }
}
