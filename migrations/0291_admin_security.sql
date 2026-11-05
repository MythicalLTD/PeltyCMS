-- Admin account security: step-up re-authentication, password age, login alerts.
ALTER TABLE sessions ADD COLUMN IF NOT EXISTS reauth_at TIMESTAMPTZ;
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_changed_at TIMESTAMPTZ NOT NULL DEFAULT now();
CREATE INDEX IF NOT EXISTS sessions_user_ip_idx ON sessions (user_id, ip);
