-- Optional domain packs (gaming store, creator marketplace, digital products, ...).
-- A fresh install has no rows: every module is off and PeltyCMS behaves as the
-- classic hosting/billing panel. Rows are written when an admin toggles one.
CREATE TABLE feature_modules (
    id          TEXT PRIMARY KEY CHECK (id ~ '^[a-z][a-z0-9_]{1,40}$'),
    enabled     BOOLEAN NOT NULL DEFAULT false,
    settings    JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
