CREATE TABLE midas.browser_sessions (
    id uuid PRIMARY KEY,
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    operator_id uuid NOT NULL REFERENCES midas.operators(id) ON DELETE CASCADE,
    credential_version bigint NOT NULL CHECK (credential_version > 0),
    authentication_method text NOT NULL CHECK (authentication_method IN ('passkey', 'password_totp', 'recovery_code')),
    authenticated_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    idle_expires_at timestamptz NOT NULL,
    idle_seconds integer NOT NULL CHECK (idle_seconds > 0)
);
CREATE INDEX browser_sessions_operator ON midas.browser_sessions(operator_id);
CREATE INDEX browser_sessions_expiry ON midas.browser_sessions(expires_at);
CREATE INDEX browser_sessions_idle ON midas.browser_sessions(idle_expires_at);
GRANT SELECT, INSERT, UPDATE, DELETE ON midas.browser_sessions TO midas_runtime;
