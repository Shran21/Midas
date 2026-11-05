CREATE FUNCTION midas.audit_hash(
    previous bytea, sequence bigint, occurred_at timestamptz, tenant_id uuid, action text,
    outcome text, actor_operator_id uuid, actor_session_id uuid, subject_operator_id uuid,
    device_id uuid, source inet, details jsonb
) RETURNS bytea
LANGUAGE sql STABLE SECURITY INVOKER SET search_path = pg_catalog AS $$
    SELECT sha256(previous || convert_to(jsonb_build_array(
        sequence, to_char(occurred_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'),
        tenant_id, action, outcome, actor_operator_id, actor_session_id, subject_operator_id,
        device_id, host(source), details
    )::text, 'UTF8'))
$$;

CREATE TABLE midas.platform_audit (
    sequence bigint PRIMARY KEY CHECK (sequence > 0),
    occurred_at timestamptz NOT NULL,
    tenant_id uuid,
    action text NOT NULL CHECK (action ~ '^[a-z]+(\.[a-z_]+)+$'),
    outcome text NOT NULL CHECK (outcome IN ('success', 'denied', 'failure')),
    actor_operator_id uuid,
    actor_session_id uuid,
    subject_operator_id uuid,
    device_id uuid,
    source inet,
    details jsonb NOT NULL CHECK (jsonb_typeof(details) = 'object' AND octet_length(details::text) <= 2048),
    previous_hash bytea NOT NULL CHECK (octet_length(previous_hash) = 32),
    hash bytea NOT NULL CHECK (octet_length(hash) = 32)
);
CREATE INDEX platform_audit_time ON midas.platform_audit(occurred_at);
CREATE INDEX platform_audit_actor ON midas.platform_audit(actor_operator_id, sequence);
CREATE INDEX platform_audit_subject ON midas.platform_audit(subject_operator_id, sequence);

CREATE TABLE midas.tenant_audit (
    tenant_id uuid NOT NULL REFERENCES midas.tenants(id),
    sequence bigint NOT NULL CHECK (sequence > 0),
    occurred_at timestamptz NOT NULL,
    action text NOT NULL CHECK (action ~ '^[a-z]+(\.[a-z_]+)+$'),
    outcome text NOT NULL CHECK (outcome IN ('success', 'denied', 'failure')),
    actor_operator_id uuid,
    actor_session_id uuid,
    subject_operator_id uuid,
    device_id uuid,
    source inet,
    details jsonb NOT NULL CHECK (jsonb_typeof(details) = 'object' AND octet_length(details::text) <= 2048),
    previous_hash bytea NOT NULL CHECK (octet_length(previous_hash) = 32),
    hash bytea NOT NULL CHECK (octet_length(hash) = 32),
    PRIMARY KEY (tenant_id, sequence)
);
CREATE INDEX tenant_audit_time ON midas.tenant_audit(tenant_id, occurred_at);
CREATE INDEX tenant_audit_actor ON midas.tenant_audit(tenant_id, actor_operator_id, sequence);
CREATE INDEX tenant_audit_device ON midas.tenant_audit(tenant_id, device_id, sequence);

ALTER TABLE midas.tenant_audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE midas.tenant_audit FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_isolation ON midas.tenant_audit USING (tenant_id = midas.current_tenant()) WITH CHECK (tenant_id = midas.current_tenant());

-- Chain position, time and hashes are always assigned here, never by the caller.
CREATE FUNCTION midas.audit_append() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog AS $$
DECLARE
    last_sequence bigint;
    last_hash bytea;
BEGIN
    IF TG_TABLE_NAME = 'tenant_audit' THEN
        PERFORM pg_advisory_xact_lock(hashtextextended('midas.tenant_audit:' || NEW.tenant_id::text, 0));
        SELECT a.sequence, a.hash INTO last_sequence, last_hash FROM midas.tenant_audit a
            WHERE a.tenant_id = NEW.tenant_id ORDER BY a.sequence DESC LIMIT 1;
    ELSE
        PERFORM pg_advisory_xact_lock(hashtextextended('midas.platform_audit', 0));
        SELECT a.sequence, a.hash INTO last_sequence, last_hash FROM midas.platform_audit a
            ORDER BY a.sequence DESC LIMIT 1;
    END IF;
    NEW.sequence := coalesce(last_sequence, 0) + 1;
    NEW.previous_hash := coalesce(last_hash, '\x0000000000000000000000000000000000000000000000000000000000000000'::bytea);
    NEW.occurred_at := clock_timestamp();
    NEW.hash := midas.audit_hash(NEW.previous_hash, NEW.sequence, NEW.occurred_at, NEW.tenant_id,
        NEW.action, NEW.outcome, NEW.actor_operator_id, NEW.actor_session_id,
        NEW.subject_operator_id, NEW.device_id, NEW.source, NEW.details);
    RETURN NEW;
END $$;

CREATE FUNCTION midas.audit_immutable() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog AS $$
BEGIN
    RAISE EXCEPTION 'audit records are append-only' USING ERRCODE = '42501';
END $$;

CREATE TRIGGER audit_append BEFORE INSERT ON midas.platform_audit
    FOR EACH ROW EXECUTE FUNCTION midas.audit_append();
CREATE TRIGGER audit_append BEFORE INSERT ON midas.tenant_audit
    FOR EACH ROW EXECUTE FUNCTION midas.audit_append();
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON midas.platform_audit
    FOR EACH ROW EXECUTE FUNCTION midas.audit_immutable();
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE ON midas.tenant_audit
    FOR EACH ROW EXECUTE FUNCTION midas.audit_immutable();
CREATE TRIGGER audit_no_truncate BEFORE TRUNCATE ON midas.platform_audit
    FOR EACH STATEMENT EXECUTE FUNCTION midas.audit_immutable();
CREATE TRIGGER audit_no_truncate BEFORE TRUNCATE ON midas.tenant_audit
    FOR EACH STATEMENT EXECUTE FUNCTION midas.audit_immutable();

REVOKE ALL ON FUNCTION midas.audit_hash(bytea, bigint, timestamptz, uuid, text, text, uuid, uuid, uuid, uuid, inet, jsonb),
    midas.audit_append(), midas.audit_immutable() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION midas.audit_hash(bytea, bigint, timestamptz, uuid, text, text, uuid, uuid, uuid, uuid, inet, jsonb) TO midas_runtime;
GRANT SELECT, INSERT (tenant_id, action, outcome, actor_operator_id, actor_session_id, subject_operator_id, device_id, source, details)
    ON midas.platform_audit, midas.tenant_audit TO midas_runtime;
