-- Personal API keys. Only the SHA-256 of the secret is stored; `prefix` is
-- the public, non-secret part shown in the UI. A key's effective permissions
-- are its scopes intersected with its owner's current permissions.
CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    prefix TEXT NOT NULL UNIQUE,
    key_hash BYTEA NOT NULL UNIQUE,
    scopes TEXT[] NOT NULL,
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    last_used_ip TEXT,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX api_keys_user ON api_keys (user_id) WHERE revoked_at IS NULL;
