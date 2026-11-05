-- Financial reports (revenue, MRR, churn). Admin and staff can read.
INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-000000000102', 'reports.read');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-000000000102'),
  ('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0001-000000000102');
CREATE INDEX IF NOT EXISTS services_activated_idx ON services (activated_at) WHERE activated_at IS NOT NULL;
