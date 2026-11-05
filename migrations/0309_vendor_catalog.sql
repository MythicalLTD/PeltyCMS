-- MarketConnect-style vendor resale: catalog synced from a vendor server.
CREATE TABLE vendor_catalog (
    id          UUID PRIMARY KEY,
    server_id   UUID NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    sku         TEXT NOT NULL CHECK (sku ~ '^[A-Za-z0-9_.:-]{1,100}$'),
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    cost        NUMERIC(19,4) NOT NULL CHECK (cost >= 0),
    currency    CHAR(3) NOT NULL,
    cycle       TEXT NOT NULL,
    config      JSONB NOT NULL DEFAULT '{}'::jsonb,
    available   BOOLEAN NOT NULL DEFAULT true,
    product_id  UUID REFERENCES products(id) ON DELETE SET NULL,
    markup_percent NUMERIC(9,4),
    synced_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (server_id, sku)
);
CREATE INDEX vendor_catalog_product_idx ON vendor_catalog (product_id);
