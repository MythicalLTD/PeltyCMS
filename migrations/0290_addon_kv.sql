-- Addon-private key/value store for WASM addons (host `kv-*` imports).
-- Values are sealed with the app encryption key when one is configured.
CREATE TABLE IF NOT EXISTS addon_kv (
    addon_id   TEXT        NOT NULL,
    key        TEXT        NOT NULL,
    value      BYTEA       NOT NULL,
    sealed     BOOLEAN     NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (addon_id, key)
);
