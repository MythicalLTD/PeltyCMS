-- Add funds: the client pays an invoice and the amount lands in their credit
-- balance exactly once (trigger on the paid transition, same transaction).
CREATE TABLE add_funds (
    invoice_id  UUID PRIMARY KEY REFERENCES invoices(id) ON DELETE CASCADE,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    currency    CHAR(3) NOT NULL REFERENCES currencies(code),
    amount      NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    credited_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX add_funds_customer_idx ON add_funds (customer_id, created_at DESC);

CREATE FUNCTION add_funds_on_paid() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE r add_funds%ROWTYPE;
BEGIN
    UPDATE add_funds SET credited_at = now() WHERE invoice_id = NEW.id AND credited_at IS NULL RETURNING * INTO r;
    IF FOUND THEN
        INSERT INTO credit_ledger (id, customer_id, currency, amount, kind, invoice_id, note)
        VALUES (gen_random_uuid(), r.customer_id, r.currency, r.amount, 'add_funds', r.invoice_id, 'add funds');
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER add_funds_paid AFTER UPDATE OF status ON invoices
    FOR EACH ROW WHEN (NEW.status = 'paid' AND OLD.status IS DISTINCT FROM 'paid')
    EXECUTE FUNCTION add_funds_on_paid();
