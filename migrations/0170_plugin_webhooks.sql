-- Durable, HMAC-signed event delivery to external plugin webhook URLs.
CREATE TABLE plugin_webhooks (
    plugin_id     TEXT PRIMARY KEY,
    -- Admin override of the manifest webhook_url (e.g. a staging endpoint).
    url_override  TEXT,
    -- Signing secret, sealed with PELTY_ENCRYPTION_KEY.
    secret_sealed BYTEA,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE plugin_deliveries (
    id              UUID PRIMARY KEY,
    plugin_id       TEXT NOT NULL,
    event           TEXT NOT NULL,
    payload         JSONB NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','delivered','failed')),
    attempts        INT NOT NULL DEFAULT 0,
    response_status INT,
    last_error      TEXT,
    url             TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at    TIMESTAMPTZ
);
CREATE INDEX plugin_deliveries_plugin_idx ON plugin_deliveries (plugin_id, created_at DESC);
