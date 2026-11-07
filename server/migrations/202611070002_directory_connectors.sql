INSERT INTO midas.permissions (name) VALUES ('directory.discover');
-- Built-in roles are immutable at runtime; the administrator role must still hold every permission.
ALTER TABLE midas.role_permissions DISABLE TRIGGER protect_builtin_permissions;
INSERT INTO midas.role_permissions (role_id, permission, effect)
    VALUES ('00000000-0000-4000-8000-000000000001', 'directory.discover', 'allow');
ALTER TABLE midas.role_permissions ENABLE TRIGGER protect_builtin_permissions;

CREATE TABLE midas.directory_connectors (
    tenant_id uuid PRIMARY KEY REFERENCES midas.tenants(id),
    device_id uuid NOT NULL,
    domain text NOT NULL CHECK (length(domain) BETWEEN 3 AND 253),
    url text NOT NULL CHECK (url ~* '^ldaps://' AND length(url) <= 255),
    ca_pem text NOT NULL CHECK (length(ca_pem) BETWEEN 64 AND 16384),
    base_dns text[] NOT NULL CHECK (cardinality(base_dns) BETWEEN 1 AND 10),
    revision bigint NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_by uuid REFERENCES midas.operators(id) ON DELETE SET NULL,
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_check_at timestamptz,
    last_check_revision bigint,
    last_check_ok boolean,
    last_check_objects integer CHECK (last_check_objects >= 0),
    last_check_error text CHECK (last_check_error ~ '^[a-z_]{0,32}$'),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);
ALTER TABLE midas.directory_connectors ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.directory_connectors FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.directory_connectors
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
GRANT SELECT, INSERT, UPDATE ON midas.directory_connectors TO midas_runtime;
