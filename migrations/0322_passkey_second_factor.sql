-- Passkeys as a second factor after the password (alongside or instead of TOTP).
-- A row means the user opted in; it only counts while they still have a passkey.
CREATE TABLE user_passkey_2fa (
    user_id    UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    enabled_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Passkey step of a password login.
ALTER TABLE webauthn_challenges DROP CONSTRAINT IF EXISTS webauthn_challenges_kind_check;
ALTER TABLE webauthn_challenges ADD CONSTRAINT webauthn_challenges_kind_check CHECK (kind IN ('register', 'login', 'reauth', 'discover', 'mfa'));
