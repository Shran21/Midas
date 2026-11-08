ALTER TABLE midas.devices
    ADD COLUMN capabilities text[] NOT NULL DEFAULT '{}' CHECK (cardinality(capabilities) <= 32),
    ADD COLUMN health_reported_at timestamptz;

CREATE TABLE midas.device_health (
    tenant_id uuid NOT NULL,
    device_id uuid NOT NULL,
    check_id text NOT NULL CHECK (check_id ~ '^[a-z_]{1,16}$'),
    state text NOT NULL CHECK (state IN ('ok', 'warning', 'critical', 'unavailable')),
    code text NOT NULL CHECK (code ~ '^[a-z_]{1,32}$'),
    summary text NOT NULL CHECK (length(summary) <= 256),
    subjects text[] NOT NULL DEFAULT '{}' CHECK (cardinality(subjects) <= 16),
    changed_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (tenant_id, device_id, check_id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);
ALTER TABLE midas.device_health ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.device_health FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.device_health
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.device_health TO midas_runtime;
