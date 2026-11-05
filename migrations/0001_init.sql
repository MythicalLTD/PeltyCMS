-- PeltyCMS initial schema: identity, sessions, RBAC, audit, settings.
CREATE EXTENSION IF NOT EXISTS citext;

CREATE TABLE users (
    id            UUID PRIMARY KEY,
    email         CITEXT NOT NULL UNIQUE CHECK (length(email) BETWEEN 3 AND 254 AND position('@' in email) > 1),
    password_hash TEXT NOT NULL,
    display_name  TEXT NOT NULL DEFAULT '' CHECK (length(display_name) <= 100),
    locale        TEXT CHECK (locale IS NULL OR locale ~ '^[a-z]{2}(-[A-Za-z0-9]{2,8})?$'),
    is_active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX users_created_at_idx ON users (created_at DESC);

CREATE TABLE sessions (
    id           UUID PRIMARY KEY,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash   BYTEA NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    csrf_token   TEXT NOT NULL,
    ip           TEXT,
    user_agent   TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    idle_expires_at     TIMESTAMPTZ NOT NULL,
    absolute_expires_at TIMESTAMPTZ NOT NULL,
    revoked_at   TIMESTAMPTZ
);
CREATE INDEX sessions_user_idx ON sessions (user_id) WHERE revoked_at IS NULL;

CREATE TABLE roles (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE CHECK (name ~ '^[a-z][a-z0-9_]{1,40}$'),
    description TEXT NOT NULL DEFAULT ''
);
CREATE TABLE permissions (
    id   UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE CHECK (name ~ '^[a-z][a-z0-9_.]{1,80}$')
);
CREATE TABLE role_permissions (
    role_id       UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id UUID NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);
CREATE TABLE user_roles (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE RESTRICT,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role_id)
);

CREATE TABLE audit_logs (
    id         BIGSERIAL PRIMARY KEY,
    actor_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    action     TEXT NOT NULL CHECK (action ~ '^[a-z][a-z0-9_.]{1,80}$'),
    target     TEXT,
    ip         TEXT,
    metadata   JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX audit_logs_created_idx ON audit_logs (created_at DESC);
CREATE INDEX audit_logs_actor_idx ON audit_logs (actor_id);

CREATE FUNCTION audit_logs_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- ON DELETE SET NULL of actor_id is the only permitted change.
    IF TG_OP = 'UPDATE' AND NEW.id = OLD.id AND NEW.action = OLD.action
       AND NEW.target IS NOT DISTINCT FROM OLD.target AND NEW.metadata = OLD.metadata
       AND NEW.created_at = OLD.created_at AND NEW.ip IS NOT DISTINCT FROM OLD.ip
       AND NEW.actor_id IS NULL AND OLD.actor_id IS NOT NULL THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'audit_logs is append-only';
END $$;
CREATE TRIGGER audit_logs_no_update BEFORE UPDATE OR DELETE ON audit_logs
    FOR EACH ROW EXECUTE FUNCTION audit_logs_append_only();
CREATE TRIGGER audit_logs_no_truncate BEFORE TRUNCATE ON audit_logs
    FOR EACH STATEMENT EXECUTE FUNCTION audit_logs_append_only();

CREATE TABLE system_settings (
    key        TEXT PRIMARY KEY CHECK (key ~ '^[a-z][a-z0-9_.]{1,80}$'),
    value      JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed roles & permissions.
INSERT INTO roles (id, name, description) VALUES
  ('00000000-0000-0000-0000-000000000001', 'admin', 'Full access'),
  ('00000000-0000-0000-0000-000000000002', 'staff', 'Support and billing staff'),
  ('00000000-0000-0000-0000-000000000003', 'customer', 'Client area access');
INSERT INTO permissions (id, name) VALUES
  ('00000000-0000-0000-0001-000000000001', 'admin.access'),
  ('00000000-0000-0000-0001-000000000002', 'users.read'),
  ('00000000-0000-0000-0001-000000000003', 'users.manage'),
  ('00000000-0000-0000-0001-000000000004', 'audit.read'),
  ('00000000-0000-0000-0001-000000000005', 'settings.manage'),
  ('00000000-0000-0000-0001-000000000006', 'client.access');
INSERT INTO role_permissions (role_id, permission_id)
  SELECT '00000000-0000-0000-0000-000000000001'::uuid, id FROM permissions;
INSERT INTO role_permissions (role_id, permission_id)
  SELECT '00000000-0000-0000-0000-000000000002'::uuid, id FROM permissions WHERE name IN ('admin.access','users.read','audit.read','client.access');
INSERT INTO role_permissions (role_id, permission_id)
  SELECT '00000000-0000-0000-0000-000000000003'::uuid, id FROM permissions WHERE name = 'client.access';
INSERT INTO system_settings (key, value) VALUES ('theme.active', '"default"');
