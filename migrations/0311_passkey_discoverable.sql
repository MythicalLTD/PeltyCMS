-- Usernameless (discoverable credential) passkey sign-in ceremonies.
ALTER TABLE webauthn_challenges DROP CONSTRAINT IF EXISTS webauthn_challenges_kind_check;
ALTER TABLE webauthn_challenges ADD CONSTRAINT webauthn_challenges_kind_check CHECK (kind IN ('register', 'login', 'reauth', 'discover'));
ALTER TABLE passkeys ADD COLUMN discoverable BOOLEAN;
