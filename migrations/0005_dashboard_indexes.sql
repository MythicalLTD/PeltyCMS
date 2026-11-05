-- Partial indexes backing the dashboard aggregates.
CREATE INDEX payments_applied_idx ON payments (applied_at) WHERE status = 'succeeded' AND applied_at IS NOT NULL;
CREATE INDEX services_active_idx ON services (customer_id) WHERE status = 'active';
CREATE INDEX orders_pending_idx ON orders (created_at) WHERE status = 'pending';
CREATE INDEX jobs_failed_idx ON jobs (created_at) WHERE status IN ('failed','dead');
CREATE INDEX invoices_customer_unpaid_idx ON invoices (customer_id) WHERE status = 'unpaid';
