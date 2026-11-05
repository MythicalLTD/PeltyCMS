-- Refund a gateway payment to the customer's account credit instead of back
-- through the gateway (WHMCS "Add to client's credit balance").
ALTER TABLE refunds ADD COLUMN to_credit BOOLEAN NOT NULL DEFAULT FALSE;
