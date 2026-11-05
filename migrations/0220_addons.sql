-- Addon registry: provisioning modules, payment gateways, themes and plugins
-- must be installed (and enabled) before they can run. See docs/ADDONS.md.
CREATE TABLE installed_addons (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL CHECK (kind IN ('provisioning','gateway','theme','plugin')),
    engine        TEXT NOT NULL,
    version       TEXT NOT NULL,
    source        TEXT NOT NULL DEFAULT 'builtin' CHECK (source IN ('builtin','disk')),
    enabled       BOOLEAN NOT NULL DEFAULT false,
    settings      JSONB NOT NULL DEFAULT '{}'::jsonb,
    permissions   TEXT[] NOT NULL DEFAULT '{}',
    manifest_hash TEXT NOT NULL,
    installed_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX installed_addons_engine ON installed_addons (kind, engine);

CREATE TABLE addon_migrations (
    addon_id   TEXT NOT NULL,
    name       TEXT NOT NULL,
    checksum   TEXT NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (addon_id, name)
);

-- Existing installations keep working: mark every built-in as installed and
-- enabled. A fresh database (no products, invoices or gateway rows) starts
-- with nothing installed.
INSERT INTO installed_addons (id, kind, engine, version, enabled, manifest_hash)
SELECT b.id, b.kind, b.engine, '1.0.0', true, 'builtin'
FROM (VALUES
    ('provisioning-pterodactyl','provisioning','pterodactyl'),
    ('provisioning-featherpanel','provisioning','featherpanel'),
    ('provisioning-pelican','provisioning','pelican'),
    ('provisioning-webhook','provisioning','webhook'),
    ('provisioning-https-agent','provisioning','https_agent'),
    ('gateway-manual','gateway','manual'),
    ('gateway-stripe','gateway','stripe'),
    ('gateway-paypal','gateway','paypal'),
    ('gateway-mollie','gateway','mollie'),
    ('gateway-razorpay','gateway','razorpay'),
    ('gateway-crypto','gateway','crypto'),
    ('gateway-paddle','gateway','paddle'),
    ('gateway-adyen','gateway','adyen'),
    ('gateway-custom-http','gateway','custom_http')
) AS b(id, kind, engine)
WHERE EXISTS (SELECT 1 FROM products) OR EXISTS (SELECT 1 FROM invoices) OR EXISTS (SELECT 1 FROM payment_gateways);
