ALTER TABLE midas.devices
    ADD COLUMN inventory_generation bigint,
    ADD COLUMN inventory_reconciled_at timestamptz;

CREATE TABLE midas.device_inventory (
    tenant_id uuid NOT NULL,
    device_id uuid NOT NULL,
    kind text NOT NULL CHECK (kind ~ '^[a-z_]{1,16}$'),
    key text NOT NULL CHECK (length(key) BETWEEN 1 AND 256),
    value text NOT NULL CHECK (length(value) <= 256),
    generation bigint NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (tenant_id, device_id, kind, key),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);
ALTER TABLE midas.device_inventory ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.device_inventory FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.device_inventory
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.device_inventory TO midas_runtime;
