-- Coupons / promo codes. Discounts are applied as a negative invoice line and,
-- for recurring coupons, persisted on the service's recurring amount.
CREATE TABLE coupons (
    id                    UUID PRIMARY KEY,
    code                  TEXT NOT NULL UNIQUE CHECK (code ~ '^[A-Z0-9_-]{3,40}$'),
    description           TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 500),
    kind                  TEXT NOT NULL CHECK (kind IN ('percent','fixed')),
    value                 NUMERIC(19,4) NOT NULL CHECK (value > 0),
    currency              CHAR(3) REFERENCES currencies(code),
    recurring             BOOLEAN NOT NULL DEFAULT FALSE,
    max_uses              INT CHECK (max_uses IS NULL OR max_uses > 0),
    max_uses_per_customer INT CHECK (max_uses_per_customer IS NULL OR max_uses_per_customer > 0),
    uses                  INT NOT NULL DEFAULT 0 CHECK (uses >= 0),
    starts_at             TIMESTAMPTZ,
    expires_at            TIMESTAMPTZ,
    active                BOOLEAN NOT NULL DEFAULT TRUE,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (kind <> 'percent' OR value <= 100),
    CHECK (kind <> 'fixed' OR currency IS NOT NULL),
    CHECK (max_uses IS NULL OR uses <= max_uses)
);

-- Empty = applies to every product.
CREATE TABLE coupon_products (
    coupon_id  UUID NOT NULL REFERENCES coupons(id) ON DELETE CASCADE,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    PRIMARY KEY (coupon_id, product_id)
);

CREATE TABLE coupon_redemptions (
    id          UUID PRIMARY KEY,
    coupon_id   UUID NOT NULL REFERENCES coupons(id) ON DELETE RESTRICT,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    order_id    UUID NOT NULL UNIQUE REFERENCES orders(id) ON DELETE CASCADE,
    amount      NUMERIC(19,4) NOT NULL CHECK (amount >= 0),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX coupon_redemptions_coupon_customer_idx ON coupon_redemptions (coupon_id, customer_id);

ALTER TABLE orders ADD COLUMN coupon_id UUID REFERENCES coupons(id) ON DELETE RESTRICT;
ALTER TABLE orders ADD COLUMN discount NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (discount >= 0);
-- Per-item discount; recurring_discount is subtracted from the service's
-- recurring amount on approval (recurring coupons only).
ALTER TABLE order_items ADD COLUMN discount NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (discount >= 0);
ALTER TABLE order_items ADD COLUMN recurring_discount NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (recurring_discount >= 0);

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000c0001', 'coupons.manage') ON CONFLICT DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
  SELECT r.id, p.id FROM roles r, permissions p WHERE r.name = 'admin' AND p.name = 'coupons.manage'
  ON CONFLICT DO NOTHING;
