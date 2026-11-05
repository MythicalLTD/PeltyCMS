-- TOTP two-factor auth. Secrets are AES-256-GCM sealed by the app
-- (PELTY_ENCRYPTION_KEY); backup codes and challenge tokens are SHA-256 only.
CREATE TABLE user_mfa (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    secret_enc BYTEA NOT NULL,
    enabled_at TIMESTAMPTZ,
    last_step BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE user_backup_codes (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash BYTEA NOT NULL,
    used_at TIMESTAMPTZ,
    PRIMARY KEY (user_id, code_hash)
);

CREATE TABLE mfa_challenges (
    token_hash BYTEA PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    attempts INT NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX mfa_challenges_user ON mfa_challenges (user_id);
