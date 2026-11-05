-- Security / accounts suite: captcha nonces, social login (OAuth2/OIDC),
-- IP rules, admin impersonation, WHMCS-compatible API credentials,
-- versioned terms of service, client close/merge.
CREATE TABLE captcha_nonces (
    nonce      TEXT PRIMARY KEY CHECK (length(nonce) <= 64),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE oauth_states (
    state_hash BYTEA PRIMARY KEY CHECK (length(state_hash) = 32),
    provider   TEXT NOT NULL,
    verifier   TEXT NOT NULL,
    nonce      TEXT NOT NULL,
    mode       TEXT NOT NULL CHECK (mode IN ('login','link')),
    user_id    UUID REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE user_identities (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider   TEXT NOT NULL CHECK (length(provider) <= 64),
    subject    TEXT NOT NULL CHECK (length(subject) <= 255),
    email      TEXT NOT NULL DEFAULT '' CHECK (length(email) <= 254),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_login_at TIMESTAMPTZ,
    UNIQUE (provider, subject),
    UNIQUE (user_id, provider)
);

CREATE TABLE ip_rules (
    id         UUID PRIMARY KEY,
    cidr       TEXT NOT NULL CHECK (length(cidr) <= 64),
    kind       TEXT NOT NULL CHECK (kind IN ('ban','admin_allow')),
    reason     TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 500),
    expires_at TIMESTAMPTZ,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (cidr, kind)
);

ALTER TABLE sessions ADD COLUMN impersonator_id UUID REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE sessions ADD COLUMN parent_session_id UUID REFERENCES sessions(id) ON DELETE CASCADE;

CREATE TABLE tos_versions (
    id           UUID PRIMARY KEY,
    version      TEXT NOT NULL UNIQUE CHECK (length(version) BETWEEN 1 AND 40),
    url          TEXT NOT NULL DEFAULT '' CHECK (length(url) <= 500),
    summary      TEXT NOT NULL DEFAULT '' CHECK (length(summary) <= 5000),
    published_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE tos_acceptances (
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    version_id  UUID NOT NULL REFERENCES tos_versions(id) ON DELETE CASCADE,
    ip          TEXT,
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, version_id)
);

ALTER TABLE customers ADD COLUMN closed_at TIMESTAMPTZ;
ALTER TABLE customers ADD COLUMN merged_into UUID REFERENCES customers(id) ON DELETE SET NULL;

-- Captcha providers and social/SSO login providers are addons too.
ALTER TABLE installed_addons DROP CONSTRAINT IF EXISTS installed_addons_kind_check;
ALTER TABLE installed_addons ADD CONSTRAINT installed_addons_kind_check
    CHECK (kind IN ('provisioning','gateway','registrar','theme','plugin','captcha','sso','integration'));
