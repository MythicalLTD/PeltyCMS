-- One gateway checkout can settle several unpaid invoices without merging them.
CREATE TABLE payment_targets (
    payment_id UUID NOT NULL REFERENCES payments(id) ON DELETE CASCADE,
    invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE RESTRICT,
    amount     NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    PRIMARY KEY (payment_id, invoice_id)
);
CREATE INDEX payment_targets_invoice_idx ON payment_targets (invoice_id);
