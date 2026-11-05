-- Payment reversals (chargebacks): money already taken back by the bank or
-- gateway. Settled like a refund (credit note, payment/invoice status) but
-- without calling the gateway.
ALTER TABLE refunds ADD COLUMN reversal BOOLEAN NOT NULL DEFAULT false;
