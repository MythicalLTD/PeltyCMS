-- Translation editor: admin overrides on top of the shipped locale files.
CREATE TABLE translation_overrides (
    lang        TEXT NOT NULL CHECK (lang ~ '^[A-Za-z0-9-]{1,10}$'),
    key         TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 200),
    value       TEXT NOT NULL CHECK (length(value) <= 4000),
    updated_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (lang, key)
);
