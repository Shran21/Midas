CREATE TABLE midas.directories (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    id uuid NOT NULL,
    domain text NOT NULL CHECK (domain ~ '^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$'),
    url text NOT NULL CHECK (url ~ '^ldaps://[A-Za-z0-9.-]+(:[0-9]{1,5})?$'),
    base_dn text NOT NULL CHECK (length(base_dn) BETWEEN 3 AND 500),
    ca_pem text NOT NULL CHECK (length(ca_pem) BETWEEN 100 AND 65536),
    enabled boolean NOT NULL DEFAULT true,
    PRIMARY KEY (tenant_id, id)
);
-- Global routing from a UPN suffix to its tenant; it holds no directory secrets or user data.
CREATE TABLE midas.directory_domains (
    domain text PRIMARY KEY,
    tenant_id uuid NOT NULL,
    directory_id uuid NOT NULL,
    FOREIGN KEY (tenant_id, directory_id) REFERENCES midas.directories(tenant_id, id) ON DELETE CASCADE
);
CREATE TABLE midas.directory_groups (
    tenant_id uuid NOT NULL,
    id uuid NOT NULL,
    directory_id uuid NOT NULL,
    group_dn text NOT NULL CHECK (length(group_dn) BETWEEN 3 AND 500),
    role_id uuid NOT NULL REFERENCES midas.roles(id),
    site_id uuid,
    group_id uuid,
    device_id uuid,
    PRIMARY KEY (tenant_id, id),
    FOREIGN KEY (tenant_id, directory_id) REFERENCES midas.directories(tenant_id, id) ON DELETE CASCADE,
    FOREIGN KEY (tenant_id, site_id) REFERENCES midas.sites(tenant_id, id),
    FOREIGN KEY (tenant_id, group_id) REFERENCES midas.device_groups(tenant_id, id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id),
    CHECK (num_nonnulls(site_id, group_id, device_id) <= 1)
);
CREATE TABLE midas.directory_identities (
    tenant_id uuid NOT NULL,
    directory_id uuid NOT NULL,
    external_id bytea NOT NULL CHECK (octet_length(external_id) BETWEEN 1 AND 64),
    operator_id uuid NOT NULL REFERENCES midas.operators(id) ON DELETE CASCADE,
    PRIMARY KEY (tenant_id, directory_id, external_id),
    UNIQUE (operator_id),
    FOREIGN KEY (tenant_id, directory_id) REFERENCES midas.directories(tenant_id, id) ON DELETE CASCADE
);
ALTER TABLE midas.role_assignments ADD COLUMN directory_group_id uuid;
ALTER TABLE midas.role_assignments ADD FOREIGN KEY (tenant_id, directory_group_id)
    REFERENCES midas.directory_groups(tenant_id, id) ON DELETE CASCADE;

ALTER TABLE midas.directories ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.directories FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.directories USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.directory_groups ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.directory_groups FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.directory_groups USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
ALTER TABLE midas.directory_identities ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.directory_identities FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.directory_identities USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());

ALTER TABLE midas.browser_sessions DROP CONSTRAINT browser_sessions_authentication_method_check;
ALTER TABLE midas.browser_sessions ADD CONSTRAINT browser_sessions_authentication_method_check
    CHECK (authentication_method IN ('passkey', 'password_totp', 'recovery_code', 'directory'));

GRANT SELECT, INSERT, UPDATE, DELETE ON midas.directories, midas.directory_domains,
    midas.directory_groups, midas.directory_identities TO midas_runtime;
