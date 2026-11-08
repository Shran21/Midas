-- Computer objects the tenant's directory connector found; candidates until an agent proves the identity.
CREATE TABLE midas.directory_computers (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    domain text NOT NULL CHECK (domain = lower(domain) AND length(domain) BETWEEN 3 AND 253),
    object_guid uuid NOT NULL,
    name text NOT NULL CHECK (length(name) <= 256),
    dns_host_name text NOT NULL CHECK (length(dns_host_name) <= 256),
    distinguished_name text NOT NULL CHECK (length(distinguished_name) BETWEEN 1 AND 512),
    operating_system text NOT NULL CHECK (length(operating_system) <= 256),
    operating_system_version text NOT NULL CHECK (length(operating_system_version) <= 256),
    last_logon_at timestamptz,
    disabled boolean NOT NULL,
    first_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    scan bigint NOT NULL,
    removed_at timestamptz,
    PRIMARY KEY (tenant_id, domain, object_guid)
);
CREATE INDEX directory_computers_scan ON midas.directory_computers (tenant_id, scan);

ALTER TABLE midas.directory_computers ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.directory_computers FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.directory_computers
    USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());
GRANT SELECT, INSERT, UPDATE ON midas.directory_computers TO midas_runtime;
