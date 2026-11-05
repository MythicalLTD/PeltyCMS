-- PeltyCMS as an OAuth 2.0 / OpenID Connect provider.
CREATE TABLE oauth_clients (
    id            UUID PRIMARY KEY,
    client_id     TEXT NOT NULL UNIQUE,
    -- sha256 hex of the secret; NULL = public client (PKCE required)
    secret_hash   TEXT,
    name          TEXT NOT NULL,
    redirect_uris TEXT[] NOT NULL DEFAULT '{}',
    scopes        TEXT[] NOT NULL DEFAULT '{openid,profile,email}',
    -- first-party apps skip the consent screen
    trusted       BOOLEAN NOT NULL DEFAULT false,
    enabled       BOOLEAN NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE oauth_codes (
    code_hash      TEXT PRIMARY KEY,
    client_id      UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    user_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    redirect_uri   TEXT NOT NULL,
    scope          TEXT NOT NULL,
    nonce          TEXT,
    code_challenge TEXT,
    expires_at     TIMESTAMPTZ NOT NULL,
    used_at        TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE oauth_tokens (
    token_hash TEXT PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('access', 'refresh')),
    client_id  UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scope      TEXT NOT NULL,
    -- the code that minted it: replaying a used code revokes this family
    code_hash  TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX oauth_tokens_user_idx ON oauth_tokens (user_id, client_id);
CREATE INDEX oauth_tokens_code_idx ON oauth_tokens (code_hash);

CREATE TABLE oauth_consents (
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id  UUID NOT NULL REFERENCES oauth_clients(id) ON DELETE CASCADE,
    scopes     TEXT[] NOT NULL,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, client_id)
);

-- ES256 signing keys (PKCS#8, sealed with the app secret box when configured).
CREATE TABLE oauth_signing_keys (
    kid        TEXT PRIMARY KEY,
    pkcs8      BYTEA NOT NULL,
    public_x   TEXT NOT NULL,
    public_y   TEXT NOT NULL,
    active     BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
