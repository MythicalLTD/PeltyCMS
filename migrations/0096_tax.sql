-- Tax engine: rules per country/region, inclusive/exclusive pricing (global
-- setting `billing.tax`), compound taxes, EU VAT reverse charge.
CREATE TABLE tax_rules (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    country    TEXT NOT NULL DEFAULT '' CHECK (country = '' OR country ~ '^[A-Z]{2}$'),   -- '' = all
    region     TEXT NOT NULL DEFAULT '' CHECK (length(region) <= 100),                    -- '' = whole country
    rate       NUMERIC(7,4) NOT NULL CHECK (rate >= 0 AND rate <= 100),
    compound   BOOLEAN NOT NULL DEFAULT FALSE,
    is_vat     BOOLEAN NOT NULL DEFAULT TRUE,
    priority   INT NOT NULL DEFAULT 0,
    enabled    BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX tax_rules_country_idx ON tax_rules (country, region) WHERE enabled;

ALTER TABLE customers ADD COLUMN vat_number TEXT NOT NULL DEFAULT '' CHECK (vat_number = '' OR vat_number ~ '^[A-Z]{2}[A-Z0-9]{2,13}$');
ALTER TABLE customers ADD COLUMN tax_exempt BOOLEAN NOT NULL DEFAULT FALSE;

-- Tax snapshot: frozen with the invoice at finalization.
ALTER TABLE invoices ADD COLUMN tax_inclusive BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE invoices ADD COLUMN reverse_charge BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE invoices ADD COLUMN tax_breakdown JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE invoices ADD COLUMN customer_snapshot JSONB;

CREATE FUNCTION invoices_tax_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.finalized_at IS NOT NULL AND (
        NEW.tax_inclusive <> OLD.tax_inclusive OR NEW.reverse_charge <> OLD.reverse_charge
        OR NEW.tax_breakdown <> OLD.tax_breakdown OR NEW.customer_snapshot IS DISTINCT FROM OLD.customer_snapshot
    ) THEN
        RAISE EXCEPTION 'finalized invoice is immutable';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER invoices_tax_guard BEFORE UPDATE ON invoices FOR EACH ROW EXECUTE FUNCTION invoices_tax_guard();

-- Lines priced by the tax engine (vs. a manual per-line rate).
ALTER TABLE invoice_items ADD COLUMN tax_auto BOOLEAN NOT NULL DEFAULT FALSE;
