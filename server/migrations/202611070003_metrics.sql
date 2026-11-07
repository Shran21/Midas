ALTER TABLE midas.devices
    ADD COLUMN online boolean NOT NULL DEFAULT false,
    ADD COLUMN last_seen_at timestamptz,
    ADD COLUMN metrics_dropped bigint NOT NULL DEFAULT 0 CHECK (metrics_dropped >= 0);

CREATE TABLE midas.device_connectivity (
    tenant_id uuid NOT NULL,
    device_id uuid NOT NULL,
    at timestamptz NOT NULL DEFAULT clock_timestamp(),
    online boolean NOT NULL,
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);
CREATE INDEX device_connectivity_device ON midas.device_connectivity (tenant_id, device_id, at);

CREATE TABLE midas.device_metrics (
    tenant_id uuid NOT NULL,
    device_id uuid NOT NULL,
    at timestamptz NOT NULL,
    cpu_permille smallint NOT NULL CHECK (cpu_permille BETWEEN 0 AND 1000),
    memory_used bigint NOT NULL CHECK (memory_used >= 0),
    memory_total bigint NOT NULL CHECK (memory_total >= memory_used),
    disk_used bigint NOT NULL CHECK (disk_used >= 0),
    disk_total bigint NOT NULL CHECK (disk_total >= disk_used),
    net_rx bigint NOT NULL CHECK (net_rx >= 0),
    net_tx bigint NOT NULL CHECK (net_tx >= 0),
    PRIMARY KEY (tenant_id, device_id, at),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);

CREATE TABLE midas.device_metrics_hourly (
    tenant_id uuid NOT NULL,
    device_id uuid NOT NULL,
    hour timestamptz NOT NULL,
    samples integer NOT NULL CHECK (samples > 0),
    cpu_avg_permille smallint NOT NULL,
    cpu_max_permille smallint NOT NULL,
    memory_used_avg bigint NOT NULL,
    memory_used_max bigint NOT NULL,
    memory_total bigint NOT NULL,
    disk_used_max bigint NOT NULL,
    disk_total bigint NOT NULL,
    net_rx bigint NOT NULL,
    net_tx bigint NOT NULL,
    PRIMARY KEY (tenant_id, device_id, hour),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);

-- Hours below the mark are rolled up; late samples can only arrive within the ingest window.
CREATE TABLE midas.metrics_rollup_state (
    tenant_id uuid PRIMARY KEY REFERENCES midas.tenants(id),
    rolled_until timestamptz NOT NULL
);

ALTER TABLE midas.device_connectivity ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.device_connectivity FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.device_connectivity
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.device_metrics ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.device_metrics FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.device_metrics
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.device_metrics_hourly ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.device_metrics_hourly FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.device_metrics_hourly
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.metrics_rollup_state ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.metrics_rollup_state FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.metrics_rollup_state
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());

GRANT SELECT, INSERT ON midas.device_connectivity TO midas_runtime;
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.device_metrics, midas.device_metrics_hourly,
    midas.metrics_rollup_state TO midas_runtime;
