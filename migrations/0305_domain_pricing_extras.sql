-- Registrar cost sync + margin rules, premium domains, per-TLD custom fields.
ALTER TABLE tld_pricing
    ADD COLUMN cost_register NUMERIC(19,4),
    ADD COLUMN cost_renew NUMERIC(19,4),
    ADD COLUMN cost_transfer NUMERIC(19,4),
    ADD COLUMN cost_synced_at TIMESTAMPTZ;

CREATE TABLE tld_margin_rules (
    tld TEXT PRIMARY KEY,                       -- '*' = default rule
    percent NUMERIC(9,4) NOT NULL DEFAULT 0 CHECK (percent >= 0 AND percent <= 1000),
    fixed NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (fixed >= 0),
    min_margin NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (min_margin >= 0),
    rounding TEXT NOT NULL DEFAULT 'cent' CHECK (rounding IN ('cent','whole','ninety_nine')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE domains
    ADD COLUMN premium BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN premium_renew_price NUMERIC(19,4),
    ADD COLUMN extra_fields JSONB NOT NULL DEFAULT '{}'::jsonb;

CREATE TABLE tld_fields (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tld TEXT NOT NULL,
    key TEXT NOT NULL CHECK (key ~ '^[a-z][a-z0-9_]{0,39}$'),
    label TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 120),
    kind TEXT NOT NULL CHECK (kind IN ('text','select','checkbox')),
    options TEXT[] NOT NULL DEFAULT '{}',
    required BOOLEAN NOT NULL DEFAULT false,
    max_length INT NOT NULL DEFAULT 200 CHECK (max_length BETWEEN 1 AND 1000),
    sort_order INT NOT NULL DEFAULT 0,
    UNIQUE (tld, key)
);
