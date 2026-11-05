-- SAML 2.0 Service Provider: identity providers, pending AuthnRequests,
-- and the assertion replay cache.
CREATE TABLE saml_idps (
    id                     UUID PRIMARY KEY,
    name                   TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    enabled                BOOLEAN NOT NULL DEFAULT true,
    idp_entity_id          TEXT NOT NULL CHECK (length(idp_entity_id) <= 1024),
    sso_url                TEXT NOT NULL CHECK (length(sso_url) <= 2048),
    slo_url                TEXT NOT NULL DEFAULT '' CHECK (length(slo_url) <= 2048),
    idp_certs              TEXT[] NOT NULL DEFAULT '{}',
    metadata_url           TEXT NOT NULL DEFAULT '' CHECK (length(metadata_url) <= 2048),
    sp_entity_id           TEXT NOT NULL DEFAULT '' CHECK (length(sp_entity_id) <= 1024),
    sign_requests          BOOLEAN NOT NULL DEFAULT false,
    sp_key_sealed          BYTEA,
    sp_cert_pem            TEXT,
    want_assertions_signed BOOLEAN NOT NULL DEFAULT true,
    allow_idp_initiated    BOOLEAN NOT NULL DEFAULT false,
    clock_skew_secs        INT NOT NULL DEFAULT 60 CHECK (clock_skew_secs BETWEEN 0 AND 600),
    attr_email             TEXT NOT NULL DEFAULT 'email',
    attr_name              TEXT NOT NULL DEFAULT 'displayName',
    attr_first_name        TEXT NOT NULL DEFAULT 'givenName',
    attr_last_name         TEXT NOT NULL DEFAULT 'sn',
    attr_groups            TEXT NOT NULL DEFAULT 'groups',
    group_map              JSONB NOT NULL DEFAULT '{}',
    role_map               JSONB NOT NULL DEFAULT '{}',
    nameid_is_email        BOOLEAN NOT NULL DEFAULT false,
    allow_register         BOOLEAN NOT NULL DEFAULT false,
    link_by_verified_email BOOLEAN NOT NULL DEFAULT false,
    allow_staff            BOOLEAN NOT NULL DEFAULT false,
    button_label           TEXT NOT NULL DEFAULT '' CHECK (length(button_label) <= 100),
    last_test              JSONB,
    last_test_at           TIMESTAMPTZ,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE saml_requests (
    id         TEXT PRIMARY KEY CHECK (length(id) <= 100),
    idp_id     UUID NOT NULL REFERENCES saml_idps(id) ON DELETE CASCADE,
    mode       TEXT NOT NULL CHECK (mode IN ('login','link','test')),
    user_id    UUID REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE saml_replay (
    idp_id       UUID NOT NULL REFERENCES saml_idps(id) ON DELETE CASCADE,
    assertion_id TEXT NOT NULL CHECK (length(assertion_id) <= 256),
    expires_at   TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (idp_id, assertion_id)
);

-- Which IdP session a PeltyCMS session came from (for SLO).
CREATE TABLE saml_sessions (
    session_id    UUID PRIMARY KEY,
    idp_id        UUID NOT NULL REFERENCES saml_idps(id) ON DELETE CASCADE,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name_id       TEXT NOT NULL,
    session_index TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX saml_sessions_user ON saml_sessions (idp_id, name_id);
