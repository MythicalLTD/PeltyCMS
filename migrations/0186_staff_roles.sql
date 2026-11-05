-- Staff role editor: builtin roles cannot be deleted/renamed; admin keeps all permissions.
ALTER TABLE roles ADD COLUMN IF NOT EXISTS builtin BOOLEAN NOT NULL DEFAULT false;
UPDATE roles SET builtin = true WHERE name IN ('admin', 'staff', 'customer');
INSERT INTO system_settings (key, value) VALUES ('security.staff_require_2fa', 'false') ON CONFLICT (key) DO NOTHING;
