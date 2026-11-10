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

-- Credit the creator when the order's invoice is paid. Triggers fire in name
-- order, so this one ("..._0creator_...") runs before the signup-referral
-- trigger and wins for that invoice (one commission per invoice).
CREATE FUNCTION creator_code_accrue() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE a RECORD;
BEGIN
  IF NEW.status = 'paid' AND OLD.status IS DISTINCT FROM 'paid' AND NEW.kind = 'invoice'
     AND NEW.total > 0 AND NEW.order_id IS NOT NULL THEN
    SELECT af.id, af.commission_percent INTO a
      FROM creator_code_orders o
      JOIN creator_codes cc ON cc.id = o.code_id
      JOIN affiliates af ON af.id = cc.affiliate_id AND af.status = 'active'
     WHERE o.order_id = NEW.order_id;
    IF FOUND THEN
      INSERT INTO affiliate_commissions (id, affiliate_id, invoice_id, currency, amount)
      SELECT gen_random_uuid(), a.id, NEW.id, NEW.currency,
             round(NEW.total * a.commission_percent / 100, cur.exponent)
        FROM currencies cur WHERE cur.code = NEW.currency
      ON CONFLICT (invoice_id) DO NOTHING;
    END IF;
  END IF;
  RETURN NEW;
END $$;
CREATE TRIGGER invoices_affiliate_0creator_accrue AFTER UPDATE OF status ON invoices
  FOR EACH ROW EXECUTE FUNCTION creator_code_accrue();
