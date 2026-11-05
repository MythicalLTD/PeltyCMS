-- Metered / usage billing: per-product metrics, raw usage records grouped
-- into per-service periods that are invoiced when the period (cycle) ends.
CREATE TABLE usage_metrics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    key TEXT NOT NULL CHECK (key ~ '^[a-z][a-z0-9_]{0,39}$'),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    unit TEXT NOT NULL DEFAULT '' CHECK (length(unit) <= 20),
    -- Price per unit; sub-cent precision allowed, lines round to the currency.
    unit_price NUMERIC(24,8) NOT NULL CHECK (unit_price >= 0),
    included_quantity NUMERIC(24,6) NOT NULL DEFAULT 0 CHECK (included_quantity >= 0),
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (product_id, key)
);

CREATE TABLE usage_periods (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    period_start DATE NOT NULL,
    period_end DATE NOT NULL CHECK (period_end > period_start),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed', 'billed')),
    invoice_id UUID REFERENCES invoices(id) ON DELETE SET NULL,
    total NUMERIC(19,4),
    billed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (service_id, period_start)
);
-- At most one open period per service.
CREATE UNIQUE INDEX usage_periods_one_open ON usage_periods (service_id) WHERE status = 'open';
CREATE INDEX usage_periods_due ON usage_periods (period_end) WHERE status <> 'billed';

CREATE TABLE usage_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    metric_id UUID NOT NULL REFERENCES usage_metrics(id) ON DELETE CASCADE,
    period_id UUID NOT NULL REFERENCES usage_periods(id) ON DELETE CASCADE,
    quantity NUMERIC(24,6) NOT NULL CHECK (quantity > 0),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (service_id, idempotency_key)
);
CREATE INDEX usage_records_period ON usage_records (period_id, metric_id);
