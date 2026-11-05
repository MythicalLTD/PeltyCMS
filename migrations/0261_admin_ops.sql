-- Admin ops: per-staff dashboard/quick-link prefs, DB backups, custom code permission.
CREATE TABLE admin_prefs (
    user_id     UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    widgets     JSONB NOT NULL DEFAULT '[]'::jsonb,
    quick_links JSONB NOT NULL DEFAULT '[]'::jsonb,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE db_backups (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_name   TEXT NOT NULL UNIQUE CHECK (file_name ~ '^[a-z0-9_.-]{1,120}$'),
    size_bytes  BIGINT,
    status      TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','done','failed')),
    error       TEXT,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

INSERT INTO permissions (id, name) VALUES
  ('00000000-0000-0000-0001-0000000a2411', 'backups.manage'),
  ('00000000-0000-0000-0001-0000000a2412', 'settings.custom_code');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-0000000a2411'),
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-0000000a2412');
