-- Digital products (optional `digital_products` module): versioned files,
-- expiring download links, license keys with activation limits and binding,
-- and a hook for external asset delivery (e.g. FiveM escrow).
CREATE TABLE digital_products (
    product_id      UUID PRIMARY KEY REFERENCES products(id) ON DELETE CASCADE,
    license_enabled BOOLEAN NOT NULL DEFAULT true,
    -- 0 = unlimited activations.
    max_activations INT NOT NULL DEFAULT 1 CHECK (max_activations BETWEEN 0 AND 1000),
    bind_domain     BOOLEAN NOT NULL DEFAULT false,
    bind_ip         BOOLEAN NOT NULL DEFAULT false,
    bind_hwid       BOOLEAN NOT NULL DEFAULT false,
    -- NULL = license lives as long as the service.
    license_days    INT CHECK (license_days IS NULL OR license_days BETWEEN 1 AND 36500),
    link_ttl_secs   INT NOT NULL DEFAULT 900 CHECK (link_ttl_secs BETWEEN 60 AND 86400),
    escrow_asset    TEXT NOT NULL DEFAULT '' CHECK (length(escrow_asset) <= 200),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE digital_versions (
    id           UUID PRIMARY KEY,
    product_id   UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    version      TEXT NOT NULL CHECK (version ~ '^[0-9A-Za-z][0-9A-Za-z.+_-]{0,39}$'),
    changelog    TEXT NOT NULL DEFAULT '' CHECK (length(changelog) <= 20000),
    filename     TEXT NOT NULL CHECK (length(filename) BETWEEN 1 AND 120),
    content_type TEXT NOT NULL,
    size_bytes   BIGINT NOT NULL CHECK (size_bytes > 0),
    sha256       BYTEA NOT NULL,
    data         BYTEA NOT NULL,
    downloads    INT NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (product_id, version)
);
CREATE INDEX digital_versions_product ON digital_versions (product_id, created_at DESC);

CREATE TABLE software_licenses (
    id              UUID PRIMARY KEY,
    product_id      UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    customer_id     UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    service_id      UUID UNIQUE REFERENCES services(id) ON DELETE SET NULL,
    key_hash        BYTEA NOT NULL UNIQUE,
    key_sealed      BYTEA NOT NULL,
    key_hint        TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'suspended', 'revoked')),
    max_activations INT NOT NULL CHECK (max_activations BETWEEN 0 AND 1000),
    expires_at      TIMESTAMPTZ,
    note            TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX software_licenses_customer ON software_licenses (customer_id);

CREATE TABLE license_activations (
    id          UUID PRIMARY KEY,
    license_id  UUID NOT NULL REFERENCES software_licenses(id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL,
    domain      TEXT NOT NULL DEFAULT '',
    ip          TEXT NOT NULL DEFAULT '',
    hwid        TEXT NOT NULL DEFAULT '',
    first_seen  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (license_id, fingerprint)
);

CREATE TABLE digital_download_links (
    token_hash  BYTEA PRIMARY KEY,
    version_id  UUID NOT NULL REFERENCES digital_versions(id) ON DELETE CASCADE,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    expires_at  TIMESTAMPTZ NOT NULL,
    uses        INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX digital_download_links_expiry ON digital_download_links (expires_at);

CREATE TABLE digital_asset_grants (
    id          UUID PRIMARY KEY,
    product_id  UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    service_id  UUID REFERENCES services(id) ON DELETE SET NULL,
    asset       TEXT NOT NULL,
    identity    TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'granted', 'revoked')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (service_id, asset)
);
