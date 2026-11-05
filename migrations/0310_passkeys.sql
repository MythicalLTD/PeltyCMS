-- Passkeys (WebAuthn). The credential is the serialized webauthn-rs Passkey
-- (public key + counter only; no secret material).
CREATE TABLE passkeys (
    id           UUID PRIMARY KEY,
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    cred_id      TEXT NOT NULL UNIQUE,
    credential   JSONB NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ
);
CREATE INDEX passkeys_user_idx ON passkeys (user_id);

-- Single-use ceremony state (registration / login / step-up), 5 minutes.
CREATE TABLE webauthn_challenges (
    id         UUID PRIMARY KEY,
    user_id    UUID REFERENCES users(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('register', 'login', 'reauth')),
    state      JSONB NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX webauthn_challenges_exp_idx ON webauthn_challenges (expires_at);
