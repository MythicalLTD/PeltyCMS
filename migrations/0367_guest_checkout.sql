-- Guest checkout (module `guest_checkout`): a store (game server) can let
-- buyers check out with only a player name / game identifier and an email.
ALTER TABLE game_servers ADD COLUMN guest_checkout BOOLEAN NOT NULL DEFAULT false;

-- Shadow accounts created by a guest checkout. The account has an unusable
-- random password until the buyer sets one through the emailed claim link.
CREATE TABLE guest_accounts (
    user_id    UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    claimed_at TIMESTAMPTZ
);

-- Attempts per IP and per email, for the hourly rate limit.
CREATE TABLE guest_checkout_attempts (
    id    BIGSERIAL PRIMARY KEY,
    ip    TEXT NOT NULL,
    email TEXT NOT NULL,
    at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX guest_checkout_attempts_ip ON guest_checkout_attempts (ip, at);
CREATE INDEX guest_checkout_attempts_email ON guest_checkout_attempts (email, at);
