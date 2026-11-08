INSERT INTO midas.permissions (name) VALUES ('alert.acknowledge');
-- Built-in roles are immutable at runtime; technicians acknowledge alerts too.
ALTER TABLE midas.role_permissions DISABLE TRIGGER protect_builtin_permissions;
INSERT INTO midas.role_permissions (role_id, permission, effect) VALUES
    ('00000000-0000-4000-8000-000000000001', 'alert.acknowledge', 'allow'),
    ('00000000-0000-4000-8000-000000000002', 'alert.acknowledge', 'allow');
ALTER TABLE midas.role_permissions ENABLE TRIGGER protect_builtin_permissions;

CREATE TABLE midas.alerts (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    id uuid NOT NULL,
    device_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind ~ '^[a-z]+(\.[a-z_]+)*$' AND length(kind) <= 64),
    severity text NOT NULL CHECK (severity IN ('warning', 'critical')),
    summary text NOT NULL CHECK (length(summary) <= 512),
    opened_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    occurrences integer NOT NULL DEFAULT 1 CHECK (occurrences >= 1),
    resolved_at timestamptz,
    acknowledged_at timestamptz,
    acknowledged_by uuid REFERENCES midas.operators(id) ON DELETE SET NULL,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);
-- One open alert per device and condition; repeats update it instead of adding new ones.
CREATE UNIQUE INDEX alerts_one_open ON midas.alerts (tenant_id, device_id, kind) WHERE resolved_at IS NULL;
CREATE INDEX alerts_recent ON midas.alerts (tenant_id, opened_at DESC);

CREATE TABLE midas.alert_recipients (
    tenant_id uuid PRIMARY KEY REFERENCES midas.tenants(id),
    emails text[] NOT NULL CHECK (cardinality(emails) <= 20),
    updated_by uuid NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE midas.alert_notifications (
    tenant_id uuid NOT NULL,
    id uuid NOT NULL,
    alert_id uuid NOT NULL,
    recipient text NOT NULL CHECK (length(recipient) BETWEEN 3 AND 254),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'sent', 'failed')),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_error text CHECK (length(last_error) <= 64),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    sent_at timestamptz,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, alert_id) REFERENCES midas.alerts(tenant_id, id)
);
CREATE INDEX alert_notifications_due ON midas.alert_notifications (tenant_id, next_attempt_at) WHERE status = 'pending';

ALTER TABLE midas.alerts ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.alerts FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.alerts
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.alert_recipients ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.alert_recipients FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.alert_recipients
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.alert_notifications ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.alert_notifications FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.alert_notifications
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.alerts, midas.alert_recipients, midas.alert_notifications TO midas_runtime;
