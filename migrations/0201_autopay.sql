-- Saved payment methods (provider tokens only, never card data), auto-pay
-- opt-in per customer and the off-session charge attempts with retries.
CREATE TABLE payment_methods (
    id          UUID PRIMARY KEY,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    gateway     TEXT NOT NULL,
    token       TEXT NOT NULL,
    brand       TEXT NOT NULL,
    last4       TEXT NOT NULL CHECK (last4 ~ '^[0-9]{0,4}$'),
    exp_month   SMALLINT NOT NULL CHECK (exp_month BETWEEN 1 AND 12),
    exp_year    SMALLINT NOT NULL,
    is_default  BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ
);
CREATE INDEX payment_methods_customer_idx ON payment_methods (customer_id) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX payment_methods_default_idx ON payment_methods (customer_id) WHERE is_default AND deleted_at IS NULL;

ALTER TABLE customers ADD COLUMN auto_pay BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE autopay_attempts (
    id                UUID PRIMARY KEY,
    invoice_id        UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    customer_id       UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    payment_method_id UUID REFERENCES payment_methods(id) ON DELETE SET NULL,
    payment_id        UUID REFERENCES payments(id) ON DELETE SET NULL,
    attempt           INT NOT NULL CHECK (attempt >= 0),
    status            TEXT NOT NULL CHECK (status IN ('processing','succeeded','pending','declined','error')),
    error             TEXT,
    next_retry        DATE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (invoice_id, attempt)
);
CREATE INDEX autopay_attempts_customer_idx ON autopay_attempts (customer_id, created_at DESC);
