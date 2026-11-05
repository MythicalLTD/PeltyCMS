-- Email: operator-editable templates, password resets, email verification.
CREATE TABLE email_templates (
    name        TEXT NOT NULL,
    locale      TEXT NOT NULL,
    subject     TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 300),
    body        TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 20000),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (name, locale)
);

ALTER TABLE users ADD COLUMN email_verified_at TIMESTAMPTZ;
-- Accounts that existed before verification shipped are trusted.
UPDATE users SET email_verified_at = created_at;

-- One-time tokens (stored as SHA-256 hashes only).
CREATE TABLE user_tokens (
    token_hash  BYTEA PRIMARY KEY CHECK (length(token_hash) = 32),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose     TEXT NOT NULL CHECK (purpose IN ('password_reset', 'email_verify')),
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX user_tokens_user_idx ON user_tokens (user_id, purpose) WHERE used_at IS NULL;
