-- Fraud screening results for client orders (built-in rules + plugin hooks).
CREATE TABLE fraud_checks (
    id          UUID PRIMARY KEY,
    order_id    UUID REFERENCES orders(id) ON DELETE SET NULL,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    ip          TEXT,
    score       SMALLINT NOT NULL CHECK (score BETWEEN 0 AND 100),
    verdict     TEXT NOT NULL CHECK (verdict IN ('accept','review','reject')),
    reasons     JSONB NOT NULL DEFAULT '[]',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX fraud_checks_ip_idx ON fraud_checks (ip, created_at DESC) WHERE ip IS NOT NULL;
CREATE INDEX fraud_checks_customer_idx ON fraud_checks (customer_id, created_at DESC);
CREATE INDEX fraud_checks_verdict_idx ON fraud_checks (verdict, created_at DESC);
