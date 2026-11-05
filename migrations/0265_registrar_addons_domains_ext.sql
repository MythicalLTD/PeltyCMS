-- Registrars become addons (kind 'registrar'); domain pricing gets grace /
-- redemption fees and add-on prices; DNS records and domain lifecycle states.
ALTER TABLE installed_addons DROP CONSTRAINT IF EXISTS installed_addons_kind_check;
ALTER TABLE installed_addons ADD CONSTRAINT installed_addons_kind_check
    CHECK (kind IN ('provisioning','gateway','registrar','theme','plugin'));

-- The in-memory mock registrar stays usable for demos and existing installs.
INSERT INTO installed_addons (id, kind, engine, version, enabled, manifest_hash)
VALUES ('registrar-mock', 'registrar', 'mock', '1.0.0', true, 'builtin')
ON CONFLICT (id) DO NOTHING;

ALTER TABLE tld_pricing
    ADD COLUMN grace_days INT NOT NULL DEFAULT 30 CHECK (grace_days BETWEEN 0 AND 90),
    ADD COLUMN grace_fee NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (grace_fee >= 0),
    ADD COLUMN redemption_days INT NOT NULL DEFAULT 30 CHECK (redemption_days BETWEEN 0 AND 90),
    ADD COLUMN redemption_fee NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (redemption_fee >= 0),
    ADD COLUMN id_protection_price NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (id_protection_price >= 0),
    ADD COLUMN dns_management_price NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (dns_management_price >= 0),
    ADD COLUMN epp_required BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN tld_group TEXT NOT NULL DEFAULT '' CHECK (tld_group IN ('', 'hot', 'new', 'sale')),
    ADD COLUMN sort_order INT NOT NULL DEFAULT 0;

ALTER TABLE domains DROP CONSTRAINT IF EXISTS domains_status_check;
ALTER TABLE domains ADD CONSTRAINT domains_status_check
    CHECK (status IN ('pending', 'active', 'grace', 'redemption', 'expired', 'transferring', 'transferred_away', 'cancelled', 'failed', 'fraud'));
ALTER TABLE domains
    ADD COLUMN dns_management BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN registrar_lock BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN admin_notes TEXT NOT NULL DEFAULT '',
    ADD COLUMN synced_at TIMESTAMPTZ;

CREATE TABLE domain_dns_records (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    domain_id UUID NOT NULL REFERENCES domains(id) ON DELETE CASCADE,
    rtype TEXT NOT NULL CHECK (rtype IN ('A','AAAA','CNAME','MX','TXT','SRV','CAA','NS','URL','FRAME')),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    value TEXT NOT NULL CHECK (length(value) BETWEEN 1 AND 2048),
    ttl INT NOT NULL DEFAULT 3600 CHECK (ttl BETWEEN 60 AND 604800),
    priority INT CHECK (priority BETWEEN 0 AND 65535),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX domain_dns_records_domain ON domain_dns_records (domain_id);

-- Restore fees charged for a grace/redemption renewal (one per term).
ALTER TABLE domain_renewals ADD COLUMN late_fee NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (late_fee >= 0);
