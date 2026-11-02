CREATE TABLE midas.operators (
    id uuid PRIMARY KEY,
    login text NOT NULL UNIQUE CHECK (login ~ '^[a-z0-9.@_+\-]{1,254}$'),
    display_name text NOT NULL CHECK (length(btrim(display_name)) BETWEEN 1 AND 200),
    enabled boolean NOT NULL DEFAULT true,
    credential_version bigint NOT NULL DEFAULT 1 CHECK (credential_version > 0),
    password_hash text,
    totp_secret bytea,
    totp_last_step bigint NOT NULL DEFAULT -1,
    CHECK ((password_hash IS NULL) = (totp_secret IS NULL))
);

CREATE TABLE midas.passkeys (
    credential_id bytea PRIMARY KEY,
    operator_id uuid NOT NULL REFERENCES midas.operators(id) ON DELETE CASCADE,
    credential text NOT NULL,
    signature_counter bigint NOT NULL DEFAULT 0 CHECK (signature_counter >= 0)
);
CREATE INDEX passkeys_operator ON midas.passkeys(operator_id);

CREATE TABLE midas.recovery_codes (
    operator_id uuid NOT NULL REFERENCES midas.operators(id) ON DELETE CASCADE,
    digest bytea NOT NULL CHECK (octet_length(digest) = 32),
    PRIMARY KEY (operator_id, digest)
);

CREATE TABLE midas.authentication_attempts (
    bucket bytea PRIMARY KEY CHECK (octet_length(bucket) = 32),
    attempts integer NOT NULL CHECK (attempts > 0),
    expires_at timestamptz NOT NULL
);
CREATE INDEX authentication_attempts_expiry ON midas.authentication_attempts(expires_at);

GRANT SELECT, INSERT, UPDATE, DELETE ON midas.operators, midas.passkeys,
    midas.recovery_codes, midas.authentication_attempts TO midas_runtime;
