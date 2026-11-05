-- Per-gateway processing fees, amount limits and currency restrictions.
CREATE TABLE gateway_rules (
    gateway      TEXT PRIMARY KEY,
    fee_percent  NUMERIC(7,4) NOT NULL DEFAULT 0 CHECK (fee_percent >= 0 AND fee_percent <= 100),
    -- Empty = every currency allowed.
    currencies   TEXT[] NOT NULL DEFAULT '{}',
    -- {"EUR": {"min": "1.00", "max": "5000.00", "fee_fixed": "0.25"}}
    limits       JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Processing fee charged by the gateway for this payment (merchant cost).
ALTER TABLE payments ADD COLUMN fee NUMERIC(20,4);
