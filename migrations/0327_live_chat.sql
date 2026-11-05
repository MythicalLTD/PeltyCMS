-- Extend the existing polling chat without invalidating visitor credentials.
ALTER TABLE chat_sessions ADD COLUMN department_id UUID REFERENCES ticket_departments(id) ON DELETE SET NULL;
ALTER TABLE chat_sessions ADD COLUMN ticket_id UUID REFERENCES tickets(id) ON DELETE SET NULL;
CREATE INDEX chat_sessions_department_idx ON chat_sessions(department_id) WHERE status = 'open';
CREATE UNIQUE INDEX chat_sessions_ticket_idx ON chat_sessions(ticket_id) WHERE ticket_id IS NOT NULL;
-- Keep the legacy general-settings switch authoritative until explicitly configured.
INSERT INTO system_settings(key, value) VALUES ('support.chat', '{"enabled":true,"departments":[],"opening_hours":[]}') ON CONFLICT DO NOTHING;
