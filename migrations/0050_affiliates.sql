-- Affiliates: referral codes, attribution by tracking cookie at signup,
-- commissions accrued (exact decimals) when a referred customer's invoice is
-- paid, and payout requests against the available balance.
CREATE TABLE affiliates (
    id                 UUID PRIMARY KEY,
    user_id            UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    code               TEXT NOT NULL UNIQUE CHECK (code ~ '^[a-z0-9]{6,32}$'),
    commission_percent NUMERIC(7,4) NOT NULL DEFAULT 10 CHECK (commission_percent >= 0 AND commission_percent <= 100),
    status             TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','suspended')),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE affiliate_referrals (
    user_id      UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    affiliate_id UUID NOT NULL REFERENCES affiliates(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX affiliate_referrals_aff_idx ON affiliate_referrals (affiliate_id);

CREATE TABLE affiliate_commissions (
    id           UUID PRIMARY KEY,
    affiliate_id UUID NOT NULL REFERENCES affiliates(id) ON DELETE CASCADE,
    invoice_id   UUID NOT NULL UNIQUE REFERENCES invoices(id) ON DELETE RESTRICT,
    currency     CHAR(3) NOT NULL REFERENCES currencies(code),
    amount       NUMERIC(19,4) NOT NULL CHECK (amount >= 0),
    status       TEXT NOT NULL DEFAULT 'available' CHECK (status IN ('available','void')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX affiliate_commissions_aff_idx ON affiliate_commissions (affiliate_id, created_at DESC);

CREATE TABLE affiliate_payouts (
    id           UUID PRIMARY KEY,
    affiliate_id UUID NOT NULL REFERENCES affiliates(id) ON DELETE CASCADE,
    currency     CHAR(3) NOT NULL REFERENCES currencies(code),
    amount       NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    status       TEXT NOT NULL DEFAULT 'requested' CHECK (status IN ('requested','paid','rejected')),
    note         TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 1000),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ
);
CREATE INDEX affiliate_payouts_aff_idx ON affiliate_payouts (affiliate_id, created_at DESC);

-- Accrue once per invoice, rounded to the currency exponent.
CREATE FUNCTION affiliate_accrue() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE a RECORD;
BEGIN
  IF NEW.status = 'paid' AND OLD.status IS DISTINCT FROM 'paid' AND NEW.kind = 'invoice' AND NEW.total > 0 THEN
    SELECT af.id, af.commission_percent INTO a
      FROM customers c
      JOIN affiliate_referrals r ON r.user_id = c.owner_user_id
      JOIN affiliates af ON af.id = r.affiliate_id AND af.status = 'active'
     WHERE c.id = NEW.customer_id;
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
CREATE TRIGGER invoices_affiliate_accrue AFTER UPDATE OF status ON invoices
  FOR EACH ROW EXECUTE FUNCTION affiliate_accrue();

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000a0f01', 'affiliates.manage') ON CONFLICT DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
  SELECT r.id, p.id FROM roles r, permissions p WHERE r.name = 'admin' AND p.name = 'affiliates.manage'
  ON CONFLICT DO NOTHING;
