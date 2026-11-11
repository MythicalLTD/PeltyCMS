-- Game-store sign-in (addons sso-cfx, sso-steam): Cfx.re forum accounts via
-- Discourse User API keys and Steam via OpenID 2.0. Neither is OAuth 2.0,
-- so the start/callback state carries a little more.

-- Browser binding of a sign-in attempt (hash of a short-lived cookie set at
-- start) and where to send the user afterwards (a same-site path).
ALTER TABLE oauth_states ADD COLUMN bind_hash BYTEA;
ALTER TABLE oauth_states ADD COLUMN return_to TEXT CHECK (length(return_to) <= 512);

-- Display handle of the linked account (Cfx.re username, Steam persona name).
ALTER TABLE user_identities ADD COLUMN username TEXT NOT NULL DEFAULT ''
    CHECK (length(username) <= 100);

-- A verified provider identity without an e-mail (Steam, Cfx.re never share
-- one) waiting for the player to enter their e-mail to finish signing up.
CREATE TABLE sso_signups (
    token_hash BYTEA PRIMARY KEY CHECK (length(token_hash) = 32),
    provider   TEXT NOT NULL,
    subject    TEXT NOT NULL CHECK (length(subject) <= 255),
    name       TEXT NOT NULL DEFAULT '' CHECK (length(name) <= 100),
    username   TEXT NOT NULL DEFAULT '' CHECK (length(username) <= 100),
    return_to  TEXT CHECK (length(return_to) <= 512),
    expires_at TIMESTAMPTZ NOT NULL
);
