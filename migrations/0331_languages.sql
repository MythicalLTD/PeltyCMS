-- Operator-managed languages (beyond shipped locales/*.json).
-- built_in rows match the bundled files and cannot be deleted.
CREATE TABLE languages (
    code       TEXT PRIMARY KEY CHECK (code ~ '^[A-Za-z0-9-]{2,10}$'),
    name       TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    built_in   BOOLEAN NOT NULL DEFAULT false,
    enabled    BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO languages (code, name, built_in, enabled) VALUES
    ('en', 'English', true, true),
    ('ro', 'Română', true, true)
ON CONFLICT (code) DO NOTHING;
