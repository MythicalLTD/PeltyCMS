-- Refunds against individual payments (partial or full), executed through the
-- payment gateway. Each successful refund issues a credit note.
CREATE TABLE refunds (
    id              UUID PRIMARY KEY,
    payment_id      UUID NOT NULL REFERENCES payments(id) ON DELETE RESTRICT,
    invoice_id      UUID NOT NULL REFERENCES invoices(id) ON DELETE RESTRICT,
    customer_id     UUID NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    gateway         TEXT NOT NULL,
    amount          NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    currency        CHAR(3) NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('pending','succeeded','failed')),
    provider_ref    TEXT,
    reason          TEXT NOT NULL DEFAULT '',
    error           TEXT,
    idempotency_key TEXT NOT NULL UNIQUE,
    credit_note_id  UUID REFERENCES invoices(id) ON DELETE RESTRICT,
    actor_id        UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX refunds_payment_idx ON refunds (payment_id);
CREATE INDEX refunds_invoice_idx ON refunds (invoice_id);
CREATE INDEX refunds_customer_idx ON refunds (customer_id, created_at DESC);
