-- Webhook delivery log per gateway (admin troubleshooting). Bodies are not
-- stored (may contain personal data); only outcome metadata.
CREATE TABLE gateway_webhook_deliveries (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    gateway      TEXT NOT NULL,
    received_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    is_test      BOOLEAN NOT NULL DEFAULT FALSE,
    status_code  INT NOT NULL,
    signature_ok BOOLEAN NOT NULL,
    duplicate    BOOLEAN NOT NULL DEFAULT FALSE,
    event_id     TEXT,
    event_type   TEXT,
    kind         TEXT,
    provider_ref TEXT,
    error        TEXT,
    body_bytes   INT NOT NULL DEFAULT 0
);
CREATE INDEX gateway_webhook_deliveries_gw_idx ON gateway_webhook_deliveries (gateway, received_at DESC);
