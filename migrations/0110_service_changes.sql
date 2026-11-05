-- Plan changes (upgrade/downgrade with proration) and cancellation requests.
ALTER TABLE services ADD COLUMN cancel_at DATE;

CREATE TABLE service_changes (
    id             UUID PRIMARY KEY,
    service_id     UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('upgrade','downgrade')),
    from_product   UUID NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    to_product     UUID NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    old_amount     NUMERIC(19,4) NOT NULL,
    new_amount     NUMERIC(19,4) NOT NULL,
    credit         NUMERIC(19,4) NOT NULL DEFAULT 0,
    charge         NUMERIC(19,4) NOT NULL DEFAULT 0,
    period_start   DATE,
    period_end     DATE,
    invoice_id     UUID REFERENCES invoices(id) ON DELETE SET NULL,
    status         TEXT NOT NULL CHECK (status IN ('awaiting_payment','scheduled','applied','cancelled')),
    effective_date DATE,
    requested_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    applied_at     TIMESTAMPTZ
);
CREATE UNIQUE INDEX service_changes_open_idx ON service_changes (service_id) WHERE status IN ('awaiting_payment','scheduled');
CREATE INDEX service_changes_invoice_idx ON service_changes (invoice_id) WHERE status = 'awaiting_payment';

CREATE TABLE cancellation_requests (
    id             UUID PRIMARY KEY,
    service_id     UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    customer_id    UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('immediate','end_of_period')),
    reason         TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 2000),
    status         TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected','withdrawn','completed')),
    effective_date DATE,
    decision_note  TEXT NOT NULL DEFAULT '' CHECK (length(decision_note) <= 2000),
    requested_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    decided_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at     TIMESTAMPTZ,
    completed_at   TIMESTAMPTZ
);
CREATE UNIQUE INDEX cancellation_requests_open_idx ON cancellation_requests (service_id) WHERE status IN ('pending','approved');
CREATE INDEX cancellation_requests_status_idx ON cancellation_requests (status, created_at DESC);
