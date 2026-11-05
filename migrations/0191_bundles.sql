-- Product bundles (fixed set of products at a cycle, % off the first
-- invoice) and per-product upsell / cross-sell recommendations.
CREATE TABLE bundles (
    id               UUID PRIMARY KEY,
    name             TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    description      TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    active           BOOLEAN NOT NULL DEFAULT true,
    discount_percent NUMERIC(5,2) NOT NULL DEFAULT 0 CHECK (discount_percent >= 0 AND discount_percent <= 100),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE bundle_items (
    bundle_id  UUID NOT NULL REFERENCES bundles(id) ON DELETE CASCADE,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    cycle      TEXT NOT NULL,
    position   INT NOT NULL DEFAULT 0,
    PRIMARY KEY (bundle_id, product_id)
);
CREATE TABLE product_recommendations (
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    target_id  UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('upsell','cross_sell')),
    position   INT NOT NULL DEFAULT 0,
    PRIMARY KEY (product_id, target_id, kind),
    CHECK (product_id <> target_id)
);
ALTER TABLE orders ADD COLUMN bundle_id UUID REFERENCES bundles(id) ON DELETE SET NULL;
