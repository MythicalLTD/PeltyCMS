-- Duo Universal Prompt: OIDC state between redirect and callback.
CREATE TABLE duo_states (
    state_hash  BYTEA PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    nonce       TEXT NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ
);
CREATE INDEX duo_states_expires ON duo_states (expires_at);
