-- Pre-due invoice reminders (one mail per invoice and threshold).
CREATE TABLE invoice_due_reminders (
    invoice_id UUID NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    days       INT NOT NULL,
    sent_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (invoice_id, days)
);
INSERT INTO system_settings (key, value) VALUES
  ('billing.due_reminders', '{"enabled": false, "days_before": [3]}'),
  ('orders.approval', '{"auto_approve": false, "require_paid": true, "max_total": null, "existing_clients_only": false, "manual_product_ids": [], "hold_fraud_review": true}')
ON CONFLICT (key) DO NOTHING;
