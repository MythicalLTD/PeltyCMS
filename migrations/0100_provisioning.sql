-- Provisioning: panel servers, product → server binding, per-service remote state.
CREATE TABLE servers (
    id             UUID PRIMARY KEY,
    name           TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    module         TEXT NOT NULL CHECK (module ~ '^[a-z][a-z0-9_-]{1,63}$'),
    base_url       TEXT NOT NULL CHECK (base_url ~ '^https?://' AND length(base_url) <= 500),
    api_key_sealed BYTEA NOT NULL,             -- SecretBox-sealed; never returned by the API
    config         JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(config) = 'object'),
    enabled        BOOLEAN NOT NULL DEFAULT TRUE,
    max_services   INT CHECK (max_services IS NULL OR max_services > 0),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE products ADD COLUMN server_id UUID REFERENCES servers(id) ON DELETE SET NULL;

ALTER TABLE services ADD COLUMN server_id UUID REFERENCES servers(id) ON DELETE SET NULL;
ALTER TABLE services ADD COLUMN remote_id TEXT CHECK (remote_id IS NULL OR length(remote_id) <= 200);
ALTER TABLE services ADD COLUMN module_data JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE services ADD COLUMN provision_status TEXT NOT NULL DEFAULT 'none'
    CHECK (provision_status IN ('none','pending','ok','failed'));
ALTER TABLE services ADD COLUMN provision_error TEXT NOT NULL DEFAULT '' CHECK (length(provision_error) <= 2000);
ALTER TABLE services ADD COLUMN provisioned_at TIMESTAMPTZ;
CREATE INDEX services_server_idx ON services (server_id) WHERE server_id IS NOT NULL;

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000d0001', 'servers.manage') ON CONFLICT DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
  SELECT r.id, p.id FROM roles r, permissions p WHERE r.name = 'admin' AND p.name = 'servers.manage'
  ON CONFLICT DO NOTHING;
