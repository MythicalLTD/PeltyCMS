-- Plugins, payment gateways, webhook deduplication, minimal payments ledger.
CREATE TABLE plugins (
    id         TEXT PRIMARY KEY CHECK (id ~ '^[a-z][a-z0-9_-]{1,63}$'),
    enabled    BOOLEAN NOT NULL DEFAULT FALSE,
    settings   JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Gateway config. Secrets are NOT stored here: `secret_refs` maps a secret
-- name to an environment variable name (e.g. {"secret_key":"STRIPE_SECRET_KEY"}).
CREATE TABLE payment_gateways (
    id          TEXT PRIMARY KEY CHECK (id ~ '^[a-z][a-z0-9_-]{1,63}$'),
    enabled     BOOLEAN NOT NULL DEFAULT FALSE,
    config      JSONB NOT NULL DEFAULT '{}'::jsonb,
    secret_refs JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE webhook_events (
    id                UUID PRIMARY KEY,
    gateway           TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    event_type        TEXT NOT NULL,
    received_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at      TIMESTAMPTZ,
    UNIQUE (gateway, provider_event_id)
);

CREATE TABLE payments (
    id              UUID PRIMARY KEY,
    user_id         UUID REFERENCES users(id) ON DELETE SET NULL,
    gateway         TEXT NOT NULL,
    provider_ref    TEXT,
    amount          NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    currency        CHAR(3) NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    status          TEXT NOT NULL CHECK (status IN ('pending','succeeded','failed','refunded')),
    idempotency_key TEXT NOT NULL UNIQUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX payments_provider_ref_idx ON payments (gateway, provider_ref) WHERE provider_ref IS NOT NULL;

INSERT INTO permissions (id, name) VALUES
  ('00000000-0000-0000-0001-000000000007', 'plugins.manage'),
  ('00000000-0000-0000-0001-000000000008', 'gateways.manage');
INSERT INTO role_permissions (role_id, permission_id)
  SELECT '00000000-0000-0000-0000-000000000001'::uuid, id FROM permissions
  WHERE name IN ('plugins.manage','gateways.manage');
INSERT INTO payment_gateways (id, enabled, config) VALUES
  ('manual', TRUE, '{"instructions":"Transfer to IBAN XX00 0000 0000, reference = payment id"}');
