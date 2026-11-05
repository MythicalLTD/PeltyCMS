-- Example addon migration: only addon_hello_webhook_* objects are allowed.
CREATE TABLE IF NOT EXISTS addon_hello_webhook_log (
    id BIGSERIAL PRIMARY KEY,
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
