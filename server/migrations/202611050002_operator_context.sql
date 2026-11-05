CREATE FUNCTION midas.current_operator() RETURNS uuid
LANGUAGE sql STABLE SECURITY INVOKER SET search_path = pg_catalog
AS $$ SELECT nullif(current_setting('midas.operator_id', true), '')::uuid $$;
REVOKE ALL ON FUNCTION midas.current_operator() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION midas.current_operator() TO midas_runtime;

-- Read-only exception to tenant isolation: a validated operator context sees only its own memberships and their tenants.
CREATE POLICY operator_memberships ON midas.tenant_memberships FOR SELECT
    USING (operator_id = midas.current_operator());
CREATE POLICY operator_tenants ON midas.tenants FOR SELECT
    USING (EXISTS (
        SELECT 1 FROM midas.tenant_memberships m
        WHERE m.tenant_id = tenants.id AND m.operator_id = midas.current_operator() AND m.enabled
    ));
