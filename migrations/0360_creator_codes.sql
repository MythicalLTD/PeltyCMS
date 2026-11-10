-- Creator codes (module `creator_codes`): an affiliate picks a vanity code
-- that buyers enter at checkout. The order is credited to that affiliate
-- (normal commission rate) and may get an admin-set percent off the first
-- invoice.
CREATE TABLE creator_codes (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    affiliate_id     UUID NOT NULL UNIQUE REFERENCES affiliates(id) ON DELETE CASCADE,
    code             TEXT NOT NULL UNIQUE CHECK (code ~ '^[a-z0-9_-]{3,24}$'),
    discount_percent NUMERIC(5,2) NOT NULL DEFAULT 0 CHECK (discount_percent >= 0 AND discount_percent <= 50),
    enabled          BOOLEAN NOT NULL DEFAULT true,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE creator_code_orders (
    order_id   UUID PRIMARY KEY REFERENCES orders(id) ON DELETE CASCADE,
    code_id    UUID NOT NULL REFERENCES creator_codes(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX creator_code_orders_code ON creator_code_orders (code_id, created_at DESC);

ALTER TABLE carts ADD COLUMN creator_code TEXT
    CHECK (creator_code IS NULL OR length(creator_code) <= 24);

-- Commission: same rates as referrals (base %, tiers, product rates), but a
-- creator code on the order credits that affiliate instead of the buyer's
-- signup referrer. Still one commission per invoice.
CREATE OR REPLACE FUNCTION affiliate_accrue() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE a UUID; pct NUMERIC; amt NUMERIC; ex INT;
BEGIN
  IF NEW.status = 'paid' AND OLD.status IS DISTINCT FROM 'paid' AND NEW.kind = 'invoice' AND NEW.total > 0 THEN
    -- A creator code entered on the order wins over the signup referral.
    SELECT af.id INTO a
      FROM creator_code_orders o
      JOIN creator_codes cc ON cc.id = o.code_id
      JOIN affiliates af ON af.id = cc.affiliate_id AND af.status = 'active'
     WHERE NEW.order_id IS NOT NULL AND o.order_id = NEW.order_id;
    IF NOT FOUND THEN
      SELECT af.id INTO a
        FROM customers c
        JOIN affiliate_referrals r ON r.user_id = c.owner_user_id
        JOIN affiliates af ON af.id = r.affiliate_id AND af.status = 'active'
       WHERE c.id = NEW.customer_id;
    END IF;
    IF a IS NOT NULL THEN
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
