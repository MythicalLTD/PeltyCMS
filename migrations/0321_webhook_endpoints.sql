-- Outgoing webhooks: admin-managed HTTPS endpoints subscribed to domain
-- events (exact names, `prefix.*` or `*`), HMAC-signed, retried with the job
-- queue's backoff, with a per-delivery log and redelivery.
CREATE TABLE webhook_endpoints (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                 TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    url                  TEXT NOT NULL CHECK (url ~ '^https://' AND length(url) <= 500),
    events               TEXT[] NOT NULL CHECK (cardinality(events) BETWEEN 1 AND 200),
    secret_sealed        BYTEA NOT NULL,
    enabled              BOOLEAN NOT NULL DEFAULT true,
    -- Consecutive deliveries that ended failed; reset by any success.
    failure_streak       INT NOT NULL DEFAULT 0,
    disabled_reason      TEXT NOT NULL DEFAULT '' CHECK (length(disabled_reason) <= 200),
    created_by           UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_delivery_at     TIMESTAMPTZ
);

CREATE TABLE webhook_deliveries (
    id              UUID PRIMARY KEY,
    endpoint_id     UUID NOT NULL REFERENCES webhook_endpoints(id) ON DELETE CASCADE,
    event           TEXT NOT NULL,
    payload         JSONB NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','delivered','failed')),
    attempts        INT NOT NULL DEFAULT 0,
    response_status INT,
    last_error      TEXT,
    duration_ms     INT,
    redelivery_of   UUID REFERENCES webhook_deliveries(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at    TIMESTAMPTZ
);
CREATE INDEX webhook_deliveries_endpoint_idx ON webhook_deliveries (endpoint_id, created_at DESC);
CREATE INDEX webhook_deliveries_failed_idx ON webhook_deliveries (created_at DESC) WHERE status = 'failed';

INSERT INTO permissions (id, name) VALUES (gen_random_uuid(), 'webhooks.manage') ON CONFLICT (name) DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
SELECT '00000000-0000-0000-0000-000000000001', id FROM permissions WHERE name = 'webhooks.manage'
ON CONFLICT DO NOTHING;
