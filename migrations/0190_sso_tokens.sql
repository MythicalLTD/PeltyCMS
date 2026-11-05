-- One-time client SSO tokens. The token itself is signed (HMAC under the
-- server API key) and never stored; only its nonce, to burn it on use.
CREATE TABLE sso_tokens (
    nonce       TEXT PRIMARY KEY CHECK (length(nonce) BETWEEN 16 AND 64),
    service_id  UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    user_id     UUID REFERENCES users(id) ON DELETE SET NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX sso_tokens_expiry_idx ON sso_tokens (expires_at);
