-- Indexes on foreign keys that are filtered by or checked on delete
-- (ON DELETE RESTRICT / CASCADE scans) on the customer, invoice, order,
-- ticket and session paths. Found with a pg_constraint/pg_index scan; see
-- docs/PERFORMANCE.md.
CREATE INDEX IF NOT EXISTS payments_customer_idx ON payments (customer_id);
CREATE INDEX IF NOT EXISTS autopay_attempts_payment_idx ON autopay_attempts (payment_id);
CREATE INDEX IF NOT EXISTS billable_items_invoice_idx ON billable_items (invoice_id);
CREATE INDEX IF NOT EXISTS cancellation_requests_customer_idx ON cancellation_requests (customer_id);
CREATE INDEX IF NOT EXISTS carts_customer_idx ON carts (customer_id);
CREATE INDEX IF NOT EXISTS carts_order_idx ON carts (order_id);
CREATE INDEX IF NOT EXISTS chat_sessions_customer_idx ON chat_sessions (customer_id);
CREATE INDEX IF NOT EXISTS coupon_redemptions_customer_idx ON coupon_redemptions (customer_id);
CREATE INDEX IF NOT EXISTS credit_ledger_invoice_idx ON credit_ledger (invoice_id);
CREATE INDEX IF NOT EXISTS credit_ledger_payment_idx ON credit_ledger (payment_id);
CREATE INDEX IF NOT EXISTS customer_field_values_field_idx ON customer_field_values (field_id);
CREATE INDEX IF NOT EXISTS domain_renewals_invoice_idx ON domain_renewals (invoice_id);
CREATE INDEX IF NOT EXISTS domains_invoice_idx ON domains (invoice_id);
CREATE INDEX IF NOT EXISTS fraud_checks_order_idx ON fraud_checks (order_id);
CREATE INDEX IF NOT EXISTS invoices_order_idx ON invoices (order_id);
CREATE INDEX IF NOT EXISTS mail_campaign_recipients_customer_idx ON mail_campaign_recipients (customer_id);
CREATE INDEX IF NOT EXISTS oauth_codes_user_idx ON oauth_codes (user_id);
CREATE INDEX IF NOT EXISTS oauth_tokens_client_idx ON oauth_tokens (client_id);
CREATE INDEX IF NOT EXISTS order_items_product_idx ON order_items (product_id);
CREATE INDEX IF NOT EXISTS orders_invoice_idx ON orders (invoice_id);
CREATE INDEX IF NOT EXISTS refunds_credit_note_idx ON refunds (credit_note_id);
CREATE INDEX IF NOT EXISTS services_product_idx ON services (product_id);
CREATE INDEX IF NOT EXISTS sessions_parent_idx ON sessions (parent_session_id) WHERE parent_session_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS sso_tokens_user_idx ON sso_tokens (user_id);
CREATE INDEX IF NOT EXISTS sso_tokens_service_idx ON sso_tokens (service_id);
CREATE INDEX IF NOT EXISTS ticket_attachments_ticket_idx ON ticket_attachments (ticket_id);
CREATE INDEX IF NOT EXISTS ticket_escalations_ticket_idx ON ticket_escalations (ticket_id);
CREATE INDEX IF NOT EXISTS tickets_service_idx ON tickets (service_id);
CREATE INDEX IF NOT EXISTS tickets_department_idx ON tickets (department_id);
CREATE INDEX IF NOT EXISTS tickets_assigned_idx ON tickets (assigned_to) WHERE assigned_to IS NOT NULL;
CREATE INDEX IF NOT EXISTS usage_periods_invoice_idx ON usage_periods (invoice_id);
CREATE INDEX IF NOT EXISTS usage_records_metric_idx ON usage_records (metric_id);
CREATE INDEX IF NOT EXISTS user_roles_role_idx ON user_roles (role_id);
