CREATE TABLE midas.permissions (name text PRIMARY KEY);
INSERT INTO midas.permissions (name) VALUES
    ('device.read'), ('monitor.read'), ('remote.view'), ('remote.control'),
    ('remote.admin'), ('remote.uac.interact'), ('remote.unattended'),
    ('terminal.execute'), ('files.read'), ('files.write'), ('script.execute'),
    ('updates.manage'), ('domain.join'), ('access.manage'), ('audit.read');

CREATE TABLE midas.roles (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (name = btrim(name) AND length(name) BETWEEN 1 AND 100),
    builtin text UNIQUE CHECK (builtin IN ('administrator', 'technician', 'observer')),
    enabled boolean NOT NULL DEFAULT true
);
CREATE UNIQUE INDEX roles_name_unique ON midas.roles (lower(name));
CREATE TABLE midas.role_permissions (
    role_id uuid NOT NULL REFERENCES midas.roles(id) ON DELETE CASCADE,
    permission text NOT NULL REFERENCES midas.permissions(name),
    effect text NOT NULL CHECK (effect IN ('allow', 'deny')),
    PRIMARY KEY (role_id, permission)
);
INSERT INTO midas.roles (id, name, builtin) VALUES
    ('00000000-0000-4000-8000-000000000001', 'Administrator', 'administrator'),
    ('00000000-0000-4000-8000-000000000002', 'Technician', 'technician'),
    ('00000000-0000-4000-8000-000000000003', 'Observer', 'observer');
INSERT INTO midas.role_permissions SELECT '00000000-0000-4000-8000-000000000001', name, 'allow' FROM midas.permissions;
INSERT INTO midas.role_permissions SELECT '00000000-0000-4000-8000-000000000002', name, 'allow' FROM midas.permissions WHERE name IN ('device.read', 'monitor.read', 'remote.view', 'remote.control');
INSERT INTO midas.role_permissions SELECT '00000000-0000-4000-8000-000000000003', name, 'allow' FROM midas.permissions WHERE name IN ('device.read', 'monitor.read');

CREATE FUNCTION midas.protect_builtin_role() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog AS $$
BEGIN
    IF OLD.builtin IS NOT NULL THEN
        RAISE EXCEPTION 'built-in roles are immutable' USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER protect_builtin_role BEFORE UPDATE OR DELETE ON midas.roles
    FOR EACH ROW EXECUTE FUNCTION midas.protect_builtin_role();

CREATE FUNCTION midas.protect_builtin_permissions() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog AS $$
BEGIN
    IF (TG_OP <> 'INSERT' AND EXISTS (SELECT 1 FROM midas.roles WHERE id = OLD.role_id AND builtin IS NOT NULL))
        OR (TG_OP <> 'DELETE' AND EXISTS (SELECT 1 FROM midas.roles WHERE id = NEW.role_id AND builtin IS NOT NULL)) THEN
        RAISE EXCEPTION 'built-in permissions are immutable' USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER protect_builtin_permissions BEFORE INSERT OR UPDATE OR DELETE ON midas.role_permissions
    FOR EACH ROW EXECUTE FUNCTION midas.protect_builtin_permissions();
REVOKE ALL ON FUNCTION midas.protect_builtin_role(), midas.protect_builtin_permissions() FROM PUBLIC;

CREATE TABLE midas.tenant_memberships (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    operator_id uuid NOT NULL REFERENCES midas.operators(id) ON DELETE CASCADE,
    enabled boolean NOT NULL DEFAULT true,
    PRIMARY KEY (tenant_id, operator_id)
);
CREATE TABLE midas.role_assignments (
    tenant_id uuid NOT NULL,
    id uuid NOT NULL,
    operator_id uuid NOT NULL,
    role_id uuid NOT NULL REFERENCES midas.roles(id),
    site_id uuid,
    group_id uuid,
    device_id uuid,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, operator_id) REFERENCES midas.tenant_memberships(tenant_id, operator_id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, site_id) REFERENCES midas.sites(tenant_id, id),
    FOREIGN KEY (tenant_id, group_id) REFERENCES midas.device_groups(tenant_id, id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id),
    CHECK (num_nonnulls(site_id, group_id, device_id) <= 1)
);
CREATE INDEX role_assignments_operator ON midas.role_assignments(tenant_id, operator_id);
CREATE TABLE midas.permission_overrides (
    tenant_id uuid NOT NULL,
    id uuid NOT NULL,
    operator_id uuid NOT NULL,
    permission text NOT NULL REFERENCES midas.permissions(name),
    effect text NOT NULL CHECK (effect IN ('allow', 'deny')),
    site_id uuid,
    group_id uuid,
    device_id uuid,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, operator_id) REFERENCES midas.tenant_memberships(tenant_id, operator_id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, site_id) REFERENCES midas.sites(tenant_id, id),
    FOREIGN KEY (tenant_id, group_id) REFERENCES midas.device_groups(tenant_id, id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id),
    CHECK (num_nonnulls(site_id, group_id, device_id) <= 1)
);
CREATE INDEX permission_overrides_operator ON midas.permission_overrides(tenant_id, operator_id);

ALTER TABLE midas.tenant_memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.tenant_memberships FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.tenant_memberships USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.role_assignments ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.role_assignments FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.role_assignments USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.permission_overrides ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.permission_overrides FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.permission_overrides USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());

ALTER TABLE midas.tenants ADD COLUMN unattended_enabled boolean NOT NULL DEFAULT false;
ALTER TABLE midas.devices ADD COLUMN unattended_enabled boolean NOT NULL DEFAULT false;

GRANT SELECT ON midas.permissions, midas.roles TO midas_runtime;
GRANT INSERT (id, name), UPDATE (name, enabled), DELETE ON midas.roles TO midas_runtime;
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.role_permissions, midas.tenant_memberships,
    midas.role_assignments, midas.permission_overrides TO midas_runtime;
