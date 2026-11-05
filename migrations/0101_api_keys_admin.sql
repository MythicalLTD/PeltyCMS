-- Admins can list and revoke any user's API keys.
INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-000000000101', 'api_keys.manage');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-000000000101');
