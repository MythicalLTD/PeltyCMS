-- Overdue automation: one row per (invoice, action) so reminders, late fees
-- and suspensions happen exactly once even if the daily job runs twice.
CREATE TABLE overdue_actions (
    invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    action     TEXT NOT NULL CHECK (action ~ '^(reminder:[0-9]{1,3}|late_fee|suspend)$'),
    -- late_fee: the separate fee invoice.
    ref_id     UUID REFERENCES invoices(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (invoice_id, action)
);
CREATE INDEX overdue_actions_ref ON overdue_actions (ref_id) WHERE ref_id IS NOT NULL;
