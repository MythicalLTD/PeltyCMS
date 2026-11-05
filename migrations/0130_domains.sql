-- Domains: TLD price list, registered/transferred domains and renewals.
CREATE TABLE tld_pricing (
    tld TEXT NOT NULL CHECK (tld ~ '^[a-z0-9][a-z0-9.-]{0,30}$'),
    currency CHAR(3) NOT NULL,
    register_price NUMERIC(19,4) NOT NULL CHECK (register_price >= 0),
    renew_price NUMERIC(19,4) NOT NULL CHECK (renew_price >= 0),
    transfer_price NUMERIC(19,4) NOT NULL CHECK (transfer_price >= 0),
    min_years INT NOT NULL DEFAULT 1 CHECK (min_years BETWEEN 1 AND 10),
    max_years INT NOT NULL DEFAULT 10 CHECK (max_years BETWEEN 1 AND 10 AND max_years >= min_years),
    registrar TEXT NOT NULL DEFAULT 'mock',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tld, currency)
);

CREATE TABLE domains (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    tld TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('register', 'transfer')),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'active', 'expired', 'transferring', 'cancelled', 'failed')),
    registrar TEXT NOT NULL,
    years INT NOT NULL CHECK (years BETWEEN 1 AND 10),
    registered_on DATE,
    expires_on DATE,
    auto_renew BOOLEAN NOT NULL DEFAULT TRUE,
    nameservers TEXT[] NOT NULL DEFAULT '{}',
    contact JSONB NOT NULL DEFAULT '{}',
    privacy BOOLEAN NOT NULL DEFAULT FALSE,
    auth_code_sealed BYTEA,
    invoice_id UUID REFERENCES invoices(id) ON DELETE SET NULL,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- One live record per name (cancelled/failed orders may be retried).
CREATE UNIQUE INDEX domains_name_live ON domains (lower(name)) WHERE status NOT IN ('cancelled', 'failed');
CREATE INDEX domains_customer ON domains (customer_id);
CREATE INDEX domains_expiry ON domains (expires_on) WHERE status = 'active';

CREATE TABLE domain_renewals (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    domain_id UUID NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    years INT NOT NULL CHECK (years BETWEEN 1 AND 10),
    -- Expiry date the renewal extends; one renewal invoice per term.
    for_expiry DATE NOT NULL,
    invoice_id UUID REFERENCES invoices(id) ON DELETE SET NULL,
    applied_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (domain_id, for_expiry)
);

-- Expiry reminders sent (domain, days-before) per term, so each fires once.
CREATE TABLE domain_reminders (
    domain_id UUID NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    for_expiry DATE NOT NULL,
    days_before INT NOT NULL,
    sent_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (domain_id, for_expiry, days_before)
);
