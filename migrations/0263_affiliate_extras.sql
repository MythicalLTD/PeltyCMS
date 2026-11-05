-- Affiliate extras (WHMCS parity): per-product commission overrides, referral
-- count tiers and promotional materials. Product-rated invoice lines replace their share of the base commission.
CREATE TABLE affiliate_product_rates (
    product_id UUID PRIMARY KEY REFERENCES products(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('percent','fixed')),
    value      NUMERIC(19,4) NOT NULL CHECK (value >= 0),
    currency   CHAR(3) REFERENCES currencies(code),
    CHECK (kind = 'fixed' AND currency IS NOT NULL OR kind = 'percent' AND currency IS NULL AND value <= 100)
);

CREATE TABLE affiliate_tiers (
    id                 UUID PRIMARY KEY,
    name               TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    min_referrals      INT NOT NULL UNIQUE CHECK (min_referrals >= 1),
    commission_percent NUMERIC(7,4) NOT NULL CHECK (commission_percent >= 0 AND commission_percent <= 100)
);

CREATE TABLE affiliate_materials (
    id         UUID PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('banner','text')),
    title      TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    body       TEXT NOT NULL DEFAULT '' CHECK (length(body) <= 5000),
    image_url  TEXT NOT NULL DEFAULT '' CHECK (image_url = '' OR image_url ~ '^https?://' AND length(image_url) <= 1000),
    sort_order INT NOT NULL DEFAULT 0,
    active     BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Effective base percent: own rate, or the best tier reached by referral count.
CREATE FUNCTION affiliate_percent(aff UUID) RETURNS NUMERIC LANGUAGE sql STABLE AS $$
  SELECT greatest(af.commission_percent, COALESCE((
    SELECT max(t.commission_percent) FROM affiliate_tiers t
     WHERE t.min_referrals <= (SELECT count(*) FROM affiliate_referrals r WHERE r.affiliate_id = af.id)), 0))
    FROM affiliates af WHERE af.id = aff
$$;

CREATE OR REPLACE FUNCTION affiliate_accrue() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE a UUID; pct NUMERIC; amt NUMERIC; ex INT;
BEGIN
  IF NEW.status = 'paid' AND OLD.status IS DISTINCT FROM 'paid' AND NEW.kind = 'invoice' AND NEW.total > 0 THEN
    SELECT af.id INTO a
      FROM customers c
      JOIN affiliate_referrals r ON r.user_id = c.owner_user_id
      JOIN affiliates af ON af.id = r.affiliate_id AND af.status = 'active'
     WHERE c.id = NEW.customer_id;
    IF FOUND THEN
      pct := affiliate_percent(a);
      SELECT exponent INTO ex FROM currencies WHERE code = NEW.currency;
      -- Base: share of the invoice total (as before). Product-rated parts swap
      -- their base share for the product rate; a fixed rate is paid once per
      -- service (renewals) or order item (first invoice, before services exist).
      WITH parts AS (
        SELECT s.product_id, sum(it.amount) AS amount
          FROM invoice_items it JOIN services s ON s.id = it.service_id
         WHERE it.invoice_id = NEW.id GROUP BY s.id, s.product_id
        UNION ALL
        SELECT oi.product_id, oi.amount + oi.setup_fee - oi.discount
          FROM order_items oi
         WHERE oi.order_id = NEW.order_id
           AND NOT EXISTS (SELECT 1 FROM invoice_items x WHERE x.invoice_id = NEW.id AND x.service_id IS NOT NULL)
      )
      SELECT NEW.total * pct / 100 + COALESCE(sum(CASE pr.kind
               WHEN 'percent' THEN p.amount * pr.value / 100
               ELSE pr.value END - p.amount * pct / 100), 0)
        INTO amt
        FROM parts p JOIN affiliate_product_rates pr ON pr.product_id = p.product_id
       WHERE pr.kind = 'percent' OR pr.currency = NEW.currency;
      INSERT INTO affiliate_commissions (id, affiliate_id, invoice_id, currency, amount)
      VALUES (gen_random_uuid(), a, NEW.id, NEW.currency, greatest(round(amt, ex), 0))
      ON CONFLICT (invoice_id) DO NOTHING;
    END IF;
  END IF;
  RETURN NEW;
END $$;
