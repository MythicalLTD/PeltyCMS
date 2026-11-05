-- Customer credit balance as an append-only ledger (per currency). The
-- balance is the sum of entries; it never goes below zero (checked under a
-- row lock on the customer).
CREATE TABLE credit_ledger (
    id          UUID PRIMARY KEY,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    currency    CHAR(3) NOT NULL REFERENCES currencies(code),
    amount      NUMERIC(19,4) NOT NULL CHECK (amount <> 0),
    kind        TEXT NOT NULL CHECK (kind IN ('adjustment','applied')),
    invoice_id  UUID REFERENCES invoices(id) ON DELETE RESTRICT,
    payment_id  UUID REFERENCES payments(id) ON DELETE RESTRICT,
    note        TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    actor_id    UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (kind <> 'applied' OR (amount < 0 AND invoice_id IS NOT NULL AND payment_id IS NOT NULL))
);
CREATE INDEX credit_ledger_customer ON credit_ledger (customer_id, currency, created_at DESC);

CREATE FUNCTION credit_ledger_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'credit ledger is append-only';
END $$;
CREATE TRIGGER credit_ledger_guard BEFORE UPDATE OR DELETE ON credit_ledger
    FOR EACH ROW EXECUTE FUNCTION credit_ledger_append_only();
