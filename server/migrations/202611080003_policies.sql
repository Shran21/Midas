INSERT INTO midas.permissions (name) VALUES ('policy.manage');
-- Built-in roles are immutable at runtime; the administrator role must still hold every permission.
ALTER TABLE midas.role_permissions DISABLE TRIGGER protect_builtin_permissions;
INSERT INTO midas.role_permissions (role_id, permission, effect)
    VALUES ('00000000-0000-4000-8000-000000000001', 'policy.manage', 'allow');
ALTER TABLE midas.role_permissions ENABLE TRIGGER protect_builtin_permissions;

-- A device in several groups takes the higher priority's value; ties go by name, then id.
ALTER TABLE midas.device_groups
    ADD COLUMN policy_priority integer NOT NULL DEFAULT 0 CHECK (policy_priority BETWEEN -1000 AND 1000);

CREATE TABLE midas.policy_values (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    level text NOT NULL CHECK (level IN ('tenant', 'site', 'group')),
    scope_id uuid NOT NULL,
    key text NOT NULL CHECK (key ~ '^[a-z]+(\.[a-z_]+)+$' AND length(key) <= 64),
    value bigint NOT NULL,
    updated_by uuid NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (tenant_id, level, scope_id, key),
    CHECK (level <> 'tenant' OR scope_id = tenant_id)
);
ALTER TABLE midas.policy_values ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.policy_values FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.policy_values
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.policy_values TO midas_runtime;
