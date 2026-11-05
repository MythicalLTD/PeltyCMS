-- Per-product invoice generation lead time (NULL = global default) and
-- billable items: one-off or recurring charges billed to a client.
ALTER TABLE products ADD COLUMN invoice_days_before INT
    CHECK (invoice_days_before IS NULL OR invoice_days_before BETWEEN 0 AND 365);

CREATE TABLE billable_items (
    id              UUID PRIMARY KEY,
    customer_id     UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    description     TEXT NOT NULL,
    quantity        NUMERIC(12,4) NOT NULL DEFAULT 1 CHECK (quantity > 0),
    unit_price      NUMERIC(18,4) NOT NULL CHECK (unit_price >= 0),
    currency        CHAR(3) NOT NULL REFERENCES currencies(code),
    invoice_action  TEXT NOT NULL CHECK (invoice_action IN ('hold','next_invoice','invoice_now','recurring')),
    recur_cycle     TEXT,
    next_due_date   DATE,
    recur_remaining INT CHECK (recur_remaining IS NULL OR recur_remaining >= 0),
    invoice_id      UUID REFERENCES invoices(id) ON DELETE SET NULL,
    invoiced_at     TIMESTAMPTZ,
    created_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (invoice_action <> 'recurring' OR (recur_cycle IS NOT NULL))
);
CREATE INDEX billable_items_customer ON billable_items (customer_id, created_at DESC);
CREATE INDEX billable_items_recurring ON billable_items (next_due_date) WHERE invoice_action = 'recurring' AND next_due_date IS NOT NULL;
CREATE INDEX billable_items_pending ON billable_items (customer_id) WHERE invoice_id IS NULL AND invoice_action IN ('hold','next_invoice');
