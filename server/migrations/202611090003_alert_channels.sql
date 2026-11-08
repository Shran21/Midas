-- Teams and Slack channels: the webhook URL is the secret, stored only sealed with the server key.
CREATE TABLE midas.alert_channels (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    id uuid NOT NULL,
    kind text NOT NULL CHECK (kind IN ('teams', 'slack')),
    label text NOT NULL CHECK (length(btrim(label)) BETWEEN 1 AND 80),
    host text NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    sealed bytea NOT NULL CHECK (octet_length(sealed) BETWEEN 41 AND 1100),
    created_by uuid REFERENCES midas.operators(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_test_at timestamptz,
    last_test_ok boolean,
    last_error text CHECK (last_error ~ '^[a-z0-9_]{1,32}$'),
    PRIMARY KEY (tenant_id, id)
);

CREATE TABLE midas.alert_deliveries (
    tenant_id uuid NOT NULL,
    id uuid NOT NULL,
    alert_id uuid NOT NULL,
    channel_id uuid NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'failed')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_error text CHECK (last_error ~ '^[a-z0-9_]{1,32}$'),
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, alert_id) REFERENCES midas.alerts(tenant_id, id),
    FOREIGN KEY (tenant_id, channel_id) REFERENCES midas.alert_channels(tenant_id, id)
);
CREATE INDEX alert_deliveries_due ON midas.alert_deliveries (tenant_id, next_attempt_at) WHERE status = 'pending';

ALTER TABLE midas.alert_channels ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.alert_channels FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.alert_channels
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.alert_deliveries ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.alert_deliveries FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.alert_deliveries
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.alert_channels, midas.alert_deliveries TO midas_runtime;
