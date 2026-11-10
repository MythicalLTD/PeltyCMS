-- Timed sales (module `timed_sales`): percent off products and/or whole
-- product groups between two instants, applied without a code to the first
-- invoice of new orders.
CREATE TABLE sales (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    percent     NUMERIC(5,2) NOT NULL CHECK (percent > 0 AND percent <= 95),
    product_ids UUID[] NOT NULL DEFAULT '{}',
    group_ids   UUID[] NOT NULL DEFAULT '{}',
    starts_at   TIMESTAMPTZ NOT NULL,
    ends_at     TIMESTAMPTZ NOT NULL,
    enabled     BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (ends_at > starts_at),
    CHECK (cardinality(product_ids) + cardinality(group_ids) BETWEEN 1 AND 500)
);
CREATE INDEX sales_window ON sales (ends_at) WHERE enabled;
