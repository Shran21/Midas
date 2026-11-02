CREATE FUNCTION midas.current_tenant() RETURNS uuid
LANGUAGE sql STABLE SECURITY INVOKER SET search_path = pg_catalog
AS $$ SELECT nullif(current_setting('midas.tenant_id', true), '')::uuid $$;
REVOKE ALL ON FUNCTION midas.current_tenant() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION midas.current_tenant() TO midas_runtime;

CREATE TABLE midas.tenants (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE midas.sites (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    id uuid NOT NULL,
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    PRIMARY KEY (tenant_id, id)
);

CREATE TABLE midas.device_groups (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    id uuid NOT NULL,
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    PRIMARY KEY (tenant_id, id)
);

CREATE TABLE midas.devices (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    id uuid NOT NULL,
    site_id uuid,
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 200),
    kind text NOT NULL CHECK (kind IN ('workstation', 'server')),
    platform text NOT NULL CHECK (platform IN ('windows', 'linux')),
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, site_id) REFERENCES midas.sites(tenant_id, id)
);

CREATE TABLE midas.device_group_members (
    tenant_id uuid NOT NULL,
    group_id uuid NOT NULL,
    device_id uuid NOT NULL,
    PRIMARY KEY (tenant_id, group_id, device_id),
    FOREIGN KEY (tenant_id, group_id) REFERENCES midas.device_groups(tenant_id, id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);

ALTER TABLE midas.tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.tenants FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.tenants
    USING (id = midas.current_tenant()) WITH CHECK (id = midas.current_tenant());

ALTER TABLE midas.sites ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.sites FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.sites
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());

ALTER TABLE midas.device_groups ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.device_groups FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.device_groups
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());

ALTER TABLE midas.devices ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.devices FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.devices
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());

ALTER TABLE midas.device_group_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.device_group_members FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.device_group_members
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());

REVOKE ALL ON SCHEMA midas FROM PUBLIC;
GRANT USAGE ON SCHEMA midas TO midas_runtime;
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.tenants, midas.sites,
    midas.device_groups, midas.devices, midas.device_group_members TO midas_runtime;
