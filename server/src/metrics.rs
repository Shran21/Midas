use std::time::Duration;

use midas_protocol::{MAX_BATCH_SAMPLES, MetricsBatch};
use uuid::Uuid;

use crate::enrollment::{AgentIdentity, Enrollment};

/// Samples older than this are refused; rollups only revisit this window.
const INGEST_HOURS: i64 = 48;
const FUTURE_SECONDS: i64 = 300;
pub const MINUTE_RETENTION_DAYS: i32 = 30;
pub const HOURLY_RETENTION_MONTHS: i32 = 13;

fn clamp(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[derive(Default)]
struct Columns {
    at: Vec<i64>,
    cpu: Vec<i16>,
    memory_used: Vec<i64>,
    memory_total: Vec<i64>,
    disk_used: Vec<i64>,
    disk_total: Vec<i64>,
    net_rx: Vec<i64>,
    net_tx: Vec<i64>,
}

impl Columns {
    fn push(&mut self, sample: &midas_protocol::Sample) {
        self.at.push(sample.at);
        self.cpu
            .push(i16::try_from(sample.cpu_permille).unwrap_or(1000));
        self.memory_used.push(clamp(sample.memory_used));
        self.memory_total.push(clamp(sample.memory_total));
        self.disk_used.push(clamp(sample.disk_used));
        self.disk_total.push(clamp(sample.disk_total));
        self.net_rx.push(clamp(sample.net_rx));
        self.net_tx.push(clamp(sample.net_tx));
    }
}

impl Enrollment {
    pub(crate) async fn tenants(&self) -> Result<Vec<Uuid>, sqlx::Error> {
        sqlx::query_scalar("SELECT DISTINCT tenant_id FROM midas.agent_credentials")
            .fetch_all(&self.pool)
            .await
    }

    /// Records a sign of life; a device that was offline becomes online again.
    pub async fn seen(&self, identity: AgentIdentity) -> Result<(), sqlx::Error> {
        let mut tx = self.tenant_tx(identity.tenant).await?;
        let was: Option<bool> =
            sqlx::query_scalar("SELECT online FROM midas.devices WHERE id = $1 FOR UPDATE")
                .bind(identity.device)
                .fetch_optional(&mut *tx)
                .await?;
        sqlx::query("UPDATE midas.devices SET online = true, last_seen_at = clock_timestamp() WHERE id = $1")
            .bind(identity.device).execute(&mut *tx).await?;
        if was == Some(false) {
            sqlx::query("INSERT INTO midas.device_connectivity (tenant_id, device_id, online) VALUES ($1, $2, true)")
                .bind(identity.tenant).bind(identity.device).execute(&mut *tx).await?;
        }
        tx.commit().await
    }

    /// Stores a batch; resent samples are ignored, implausible ones skipped.
    pub async fn ingest(
        &self,
        identity: AgentIdentity,
        batch: &MetricsBatch,
    ) -> Result<(), sqlx::Error> {
        if batch.samples.len() > MAX_BATCH_SAMPLES {
            return Err(sqlx::Error::Protocol("metrics batch too large".into()));
        }
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let earliest = now - INGEST_HOURS * 3600;
        let mut columns = Columns::default();
        for sample in &batch.samples {
            let plausible = sample.at.rem_euclid(60) == 0
                && (earliest..=now + FUTURE_SECONDS).contains(&sample.at)
                && sample.cpu_permille <= 1000
                && sample.memory_used <= sample.memory_total
                && sample.disk_used <= sample.disk_total;
            if plausible {
                columns.push(sample);
            }
        }
        let skipped = batch.samples.len() - columns.at.len();
        if skipped > 0 {
            tracing::warn!(skipped, device = %identity.device, "implausible metrics samples skipped");
        }
        let mut tx = self.tenant_tx(identity.tenant).await?;
        sqlx::query("INSERT INTO midas.device_metrics (tenant_id, device_id, at, cpu_permille, memory_used, memory_total, disk_used, disk_total, net_rx, net_tx) SELECT $1, $2, to_timestamp(a), c, mu, mt, du, dt, rx, tx FROM unnest($3::bigint[], $4::smallint[], $5::bigint[], $6::bigint[], $7::bigint[], $8::bigint[], $9::bigint[], $10::bigint[]) AS u(a, c, mu, mt, du, dt, rx, tx) ON CONFLICT DO NOTHING")
            .bind(identity.tenant).bind(identity.device)
            .bind(&columns.at).bind(&columns.cpu).bind(&columns.memory_used).bind(&columns.memory_total).bind(&columns.disk_used).bind(&columns.disk_total).bind(&columns.net_rx).bind(&columns.net_tx)
            .execute(&mut *tx).await?;
        if batch.dropped > 0 {
            sqlx::query(
                "UPDATE midas.devices SET metrics_dropped = metrics_dropped + $2 WHERE id = $1",
            )
            .bind(identity.device)
            .bind(clamp(batch.dropped))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    /// Marks devices offline whose last sign of life is older than `silence`.
    pub async fn sweep(&self, silence: Duration) -> Result<Vec<AgentIdentity>, sqlx::Error> {
        let mut offline = Vec::new();
        for tenant in self.tenants().await? {
            let mut tx = self.tenant_tx(tenant).await?;
            let devices: Vec<Uuid> = sqlx::query_scalar("UPDATE midas.devices SET online = false WHERE online AND (last_seen_at IS NULL OR last_seen_at < clock_timestamp() - make_interval(secs => $1)) RETURNING id")
                .bind(silence.as_secs_f64()).fetch_all(&mut *tx).await?;
            for device in &devices {
                sqlx::query("INSERT INTO midas.device_connectivity (tenant_id, device_id, online) VALUES ($1, $2, false)")
                    .bind(tenant).bind(device).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            offline.extend(
                devices
                    .into_iter()
                    .map(|device| AgentIdentity { tenant, device }),
            );
        }
        Ok(offline)
    }

    /// Recomputes hourly rollups for completed hours since the last mark.
    pub async fn rollup(&self) -> Result<u64, sqlx::Error> {
        let mut rows = 0;
        for tenant in self.tenants().await? {
            let mut tx = self.tenant_tx(tenant).await?;
            let affected = sqlx::query("WITH bounds AS (SELECT date_trunc('hour', clock_timestamp()) AS upto, coalesce((SELECT rolled_until - make_interval(hours => $2) FROM midas.metrics_rollup_state WHERE tenant_id = $1), (SELECT date_trunc('hour', min(at)) FROM midas.device_metrics)) AS since) INSERT INTO midas.device_metrics_hourly (tenant_id, device_id, hour, samples, cpu_avg_permille, cpu_max_permille, memory_used_avg, memory_used_max, memory_total, disk_used_max, disk_total, net_rx, net_tx) SELECT m.tenant_id, m.device_id, date_trunc('hour', m.at), count(*), round(avg(m.cpu_permille)), max(m.cpu_permille), round(avg(m.memory_used)), max(m.memory_used), max(m.memory_total), max(m.disk_used), max(m.disk_total), sum(m.net_rx), sum(m.net_tx) FROM midas.device_metrics m, bounds b WHERE b.since IS NOT NULL AND m.at >= b.since AND m.at < b.upto GROUP BY 1, 2, 3 ON CONFLICT (tenant_id, device_id, hour) DO UPDATE SET samples = EXCLUDED.samples, cpu_avg_permille = EXCLUDED.cpu_avg_permille, cpu_max_permille = EXCLUDED.cpu_max_permille, memory_used_avg = EXCLUDED.memory_used_avg, memory_used_max = EXCLUDED.memory_used_max, memory_total = EXCLUDED.memory_total, disk_used_max = EXCLUDED.disk_used_max, disk_total = EXCLUDED.disk_total, net_rx = EXCLUDED.net_rx, net_tx = EXCLUDED.net_tx WHERE midas.device_metrics_hourly.samples <> EXCLUDED.samples")
                .bind(tenant).bind(i32::try_from(INGEST_HOURS).unwrap_or(48))
                .execute(&mut *tx).await?.rows_affected();
            sqlx::query("INSERT INTO midas.metrics_rollup_state (tenant_id, rolled_until) VALUES ($1, date_trunc('hour', clock_timestamp())) ON CONFLICT (tenant_id) DO UPDATE SET rolled_until = EXCLUDED.rolled_until")
                .bind(tenant).execute(&mut *tx).await?;
            tx.commit().await?;
            rows += affected;
        }
        Ok(rows)
    }

    /// Deletes expired data; minute samples go only once their hour is rolled up.
    pub async fn expire(
        &self,
        minute_days: i32,
        hourly_months: i32,
    ) -> Result<(u64, u64), sqlx::Error> {
        let (mut minutes, mut hours) = (0, 0);
        for tenant in self.tenants().await? {
            let mut tx = self.tenant_tx(tenant).await?;
            minutes += sqlx::query("DELETE FROM midas.device_metrics m WHERE m.at < clock_timestamp() - make_interval(days => $1) AND EXISTS (SELECT 1 FROM midas.device_metrics_hourly h WHERE h.tenant_id = m.tenant_id AND h.device_id = m.device_id AND h.hour = date_trunc('hour', m.at))")
                .bind(minute_days).execute(&mut *tx).await?.rows_affected();
            hours += sqlx::query("DELETE FROM midas.device_metrics_hourly WHERE hour < clock_timestamp() - make_interval(months => $1)")
                .bind(hourly_months).execute(&mut *tx).await?.rows_affected();
            tx.commit().await?;
        }
        Ok((minutes, hours))
    }

    /// Hourly housekeeping: roll up first, then expire.
    pub async fn maintain(&self) {
        match self.rollup().await {
            Ok(rows) => tracing::info!(rows, "metrics rolled up"),
            Err(error) => {
                tracing::error!(%error, "metrics rollup failed; nothing expired");
                return;
            }
        }
        match self
            .expire(MINUTE_RETENTION_DAYS, HOURLY_RETENTION_MONTHS)
            .await
        {
            Ok((minutes, hours)) => tracing::info!(minutes, hours, "expired metrics deleted"),
            Err(error) => tracing::error!(%error, "metrics expiry failed"),
        }
    }
}
