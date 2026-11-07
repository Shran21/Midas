INSERT INTO midas.permissions (name) VALUES ('device.enroll');
-- Built-in roles are immutable at runtime; the administrator role must still hold every permission.
ALTER TABLE midas.role_permissions DISABLE TRIGGER protect_builtin_permissions;
INSERT INTO midas.role_permissions (role_id, permission, effect)
    VALUES ('00000000-0000-4000-8000-000000000001', 'device.enroll', 'allow');
ALTER TABLE midas.role_permissions ENABLE TRIGGER protect_builtin_permissions;

ALTER TABLE midas.devices
    ADD COLUMN enrollment text NOT NULL DEFAULT 'active'
        CHECK (enrollment IN ('pending', 'active', 'rejected', 'revoked')),
    ADD COLUMN requested_at timestamptz,
    ADD COLUMN decided_by uuid REFERENCES midas.operators(id) ON DELETE SET NULL,
    ADD COLUMN decided_at timestamptz;

CREATE TABLE midas.enrollment_tokens (
    digest bytea PRIMARY KEY CHECK (octet_length(digest) = 32),
    id uuid NOT NULL UNIQUE,
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    site_id uuid,
    platform text NOT NULL CHECK (platform IN ('windows', 'linux')),
    kind text NOT NULL CHECK (kind IN ('workstation', 'server')),
    preapproved boolean NOT NULL,
    created_by uuid REFERENCES midas.operators(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    device_id uuid,
    FOREIGN KEY (tenant_id, site_id) REFERENCES midas.sites(tenant_id, id),
    CHECK (expires_at > created_at),
    CHECK ((used_at IS NULL) = (device_id IS NULL))
);

CREATE TABLE midas.enrollment_requests (
    id uuid PRIMARY KEY,
    digest bytea NOT NULL CHECK (octet_length(digest) = 32),
    tenant_id uuid NOT NULL,
    device_id uuid NOT NULL,
    csr bytea NOT NULL CHECK (octet_length(csr) BETWEEN 64 AND 4096),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    certificate_pem bytea CHECK (octet_length(certificate_pem) <= 8192),
    UNIQUE (tenant_id, device_id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);

CREATE TABLE midas.agent_credentials (
    fingerprint bytea PRIMARY KEY CHECK (octet_length(fingerprint) = 32),
    tenant_id uuid NOT NULL,
    device_id uuid NOT NULL,
    issued_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz,
    FOREIGN KEY (tenant_id, device_id) REFERENCES midas.devices(tenant_id, id)
);
CREATE INDEX agent_credentials_device ON midas.agent_credentials (tenant_id, device_id);

GRANT SELECT, INSERT, UPDATE ON midas.enrollment_tokens, midas.enrollment_requests,
    midas.agent_credentials TO midas_runtime;
