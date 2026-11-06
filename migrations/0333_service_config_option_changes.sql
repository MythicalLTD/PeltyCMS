-- Persist configurable-option selections on the service (copied from the
-- order item at activation) so mid-cycle option upgrades/downgrades can
-- re-price without going back to the original order.
ALTER TABLE services
    ADD COLUMN IF NOT EXISTS config JSONB NOT NULL DEFAULT '{}'::jsonb;

UPDATE services s
SET config = COALESCE(oi.config, '{}'::jsonb)
FROM order_items oi
WHERE oi.id = s.order_item_id
  AND (s.config = '{}'::jsonb OR s.config IS NULL);

-- Mid-cycle configurable-option changes with day-based proration
-- (same payment flow as plan upgrades: invoice → apply on pay).
CREATE TABLE IF NOT EXISTS service_option_changes (
    id             UUID PRIMARY KEY,
    service_id     UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    old_config     JSONB NOT NULL DEFAULT '{}'::jsonb,
    new_config     JSONB NOT NULL DEFAULT '{}'::jsonb,
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
CREATE UNIQUE INDEX IF NOT EXISTS service_option_changes_open_idx
    ON service_option_changes (service_id)
    WHERE status IN ('awaiting_payment','scheduled');
CREATE INDEX IF NOT EXISTS service_option_changes_invoice_idx
    ON service_option_changes (invoice_id)
    WHERE status = 'awaiting_payment';
