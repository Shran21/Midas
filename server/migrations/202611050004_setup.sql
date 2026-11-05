CREATE TABLE midas.setup_tokens (
    digest bytea PRIMARY KEY CHECK (octet_length(digest) = 32),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL,
    operator_id uuid REFERENCES midas.operators(id) ON DELETE SET NULL,
    used_at timestamptz
);
GRANT SELECT, INSERT, UPDATE ON midas.setup_tokens TO midas_runtime;
