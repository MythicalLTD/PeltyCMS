-- Admin productivity: personal "My Notes" and outbound staff notification rules
-- (Slack / Discord / Teams / generic webhook), WHMCS "Configure Notifications".
CREATE TABLE admin_my_notes (
    user_id    UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    body       TEXT NOT NULL DEFAULT '' CHECK (length(body) <= 20000),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE notification_rules (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    provider   TEXT NOT NULL CHECK (provider IN ('slack','discord','teams','webhook')),
    url        TEXT NOT NULL CHECK (url ~ '^https://' AND length(url) <= 500),
    events     TEXT[] NOT NULL DEFAULT '{}',
    enabled    BOOLEAN NOT NULL DEFAULT TRUE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000a2401', 'notifications.manage');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-0000000a2401');
