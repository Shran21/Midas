INSERT INTO midas.permissions (name) VALUES
    ('remote.clipboard'), ('remote.files.upload'), ('remote.files.download');
-- Built-in roles are immutable at runtime; the administrator role must still hold every permission.
ALTER TABLE midas.role_permissions DISABLE TRIGGER protect_builtin_permissions;
INSERT INTO midas.role_permissions (role_id, permission, effect)
    SELECT '00000000-0000-4000-8000-000000000001', name, 'allow' FROM midas.permissions
    WHERE name IN ('remote.clipboard', 'remote.files.upload', 'remote.files.download');
ALTER TABLE midas.role_permissions ENABLE TRIGGER protect_builtin_permissions;
