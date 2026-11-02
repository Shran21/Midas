SELECT
    current_setting('server_version_num')::integer / 10000 <> 18
    OR NOT pg_has_role(current_user, 'midas_runtime', 'USAGE')
    OR EXISTS (
        SELECT 1 FROM pg_roles
        WHERE (rolsuper OR rolbypassrls OR rolcreaterole OR rolcreatedb OR rolreplication OR rolname = 'midas_owner')
        AND pg_has_role(current_user, oid, 'MEMBER')
    )
    OR EXISTS (
        SELECT 1 FROM pg_database
        WHERE datname = current_database() AND pg_has_role(current_user, datdba, 'MEMBER')
    )
    OR has_schema_privilege(current_user, 'midas', 'CREATE')
    OR has_schema_privilege(current_user, 'public', 'CREATE')
    OR (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'midas' AND c.relname IN ('tenants', 'sites', 'device_groups', 'devices', 'device_group_members')
        AND c.relkind = 'r' AND c.relrowsecurity AND c.relforcerowsecurity) <> 5
    OR EXISTS (
        SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'midas' AND c.relkind = 'r'
        AND (pg_has_role(current_user, c.relowner, 'MEMBER')
            OR has_table_privilege(current_user, c.oid, 'TRUNCATE,TRIGGER,REFERENCES'))
    )
