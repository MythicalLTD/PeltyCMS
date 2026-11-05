-- Promotions: addons/domains scope, recurring for N cycles, per-client/group.
ALTER TABLE coupons ADD COLUMN recurring_cycles INT CHECK (recurring_cycles IS NULL OR recurring_cycles BETWEEN 1 AND 600);
ALTER TABLE coupons ADD COLUMN applies_addons BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE coupons ADD COLUMN applies_domains BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE coupons ADD COLUMN customer_ids UUID[] NOT NULL DEFAULT '{}';
ALTER TABLE coupons ADD COLUMN client_group_ids UUID[] NOT NULL DEFAULT '{}';
ALTER TABLE coupons ADD COLUMN tlds TEXT[] NOT NULL DEFAULT '{}';

-- Redemptions may come from a domain order (no product order row).
ALTER TABLE coupon_redemptions ALTER COLUMN order_id DROP NOT NULL;
ALTER TABLE coupon_redemptions ADD COLUMN domain_id UUID REFERENCES domains(id) ON DELETE CASCADE;
ALTER TABLE coupon_redemptions ADD CONSTRAINT coupon_redemptions_target CHECK (order_id IS NOT NULL OR domain_id IS NOT NULL);

-- Limited recurring discount: remaining discounted renewals, then the full price comes back.
ALTER TABLE order_items ADD COLUMN promo_cycles INT;
ALTER TABLE services ADD COLUMN promo_discount NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (promo_discount >= 0);
ALTER TABLE services ADD COLUMN promo_cycles_left INT CHECK (promo_cycles_left IS NULL OR promo_cycles_left >= 0);

-- Free domain with a product (WHMCS "Free Domain").
ALTER TABLE products ADD COLUMN free_domain TEXT NOT NULL DEFAULT 'none' CHECK (free_domain IN ('none','once','on_renewal'));
ALTER TABLE products ADD COLUMN free_domain_tlds TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE products ADD COLUMN free_domain_cycles TEXT[] NOT NULL DEFAULT '{}';
ALTER TABLE domains ADD COLUMN free_with_service UUID REFERENCES services(id) ON DELETE SET NULL;
ALTER TABLE domains ADD COLUMN free_with_order_item UUID REFERENCES order_items(id) ON DELETE SET NULL;
CREATE UNIQUE INDEX domains_free_service_once ON domains (free_with_service) WHERE free_with_service IS NOT NULL AND status NOT IN ('cancelled','failed');
CREATE UNIQUE INDEX domains_free_item_once ON domains (free_with_order_item) WHERE free_with_order_item IS NOT NULL AND status NOT IN ('cancelled','failed');

-- Order form templates.
CREATE TABLE order_forms (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug         TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,63}$'),
    name         TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    layout       TEXT NOT NULL DEFAULT 'cards' CHECK (layout IN ('cards','list','compact','wizard')),
    product_ids  UUID[] NOT NULL DEFAULT '{}',
    group_ids    UUID[] NOT NULL DEFAULT '{}',
    steps        TEXT[] NOT NULL DEFAULT '{product,configure,domain,review,checkout}',
    fields       JSONB NOT NULL DEFAULT '[]',
    redirect_url TEXT CHECK (redirect_url IS NULL OR length(redirect_url) <= 500),
    active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE product_groups ADD COLUMN order_form_id UUID REFERENCES order_forms(id) ON DELETE SET NULL;
ALTER TABLE orders ADD COLUMN order_form_id UUID REFERENCES order_forms(id) ON DELETE SET NULL;
ALTER TABLE orders ADD COLUMN form_answers JSONB NOT NULL DEFAULT '{}';
