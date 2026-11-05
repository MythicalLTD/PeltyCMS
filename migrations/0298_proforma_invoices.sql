-- Proforma invoices: an issued invoice may stay unnumbered until it is paid.
ALTER TABLE invoices DROP CONSTRAINT invoices_check;
ALTER TABLE invoices ADD CONSTRAINT invoices_number_finalized CHECK (number IS NULL OR finalized_at IS NOT NULL);

-- A proforma invoice may receive its number (and issue date) exactly once, when paid.
CREATE OR REPLACE FUNCTION invoices_guard() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE numbering BOOLEAN;
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.finalized_at IS NOT NULL THEN RAISE EXCEPTION 'finalized invoice cannot be deleted'; END IF;
        RETURN OLD;
    END IF;
    numbering := OLD.number IS NULL AND NEW.number IS NOT NULL AND NEW.status = 'paid';
    IF OLD.finalized_at IS NOT NULL AND (
        (NEW.number IS DISTINCT FROM OLD.number AND NOT numbering) OR NEW.kind <> OLD.kind
        OR NEW.credit_for IS DISTINCT FROM OLD.credit_for OR NEW.customer_id <> OLD.customer_id
        OR NEW.currency <> OLD.currency OR NEW.subtotal <> OLD.subtotal OR NEW.tax_total <> OLD.tax_total
        OR NEW.total <> OLD.total OR (NEW.issued_at IS DISTINCT FROM OLD.issued_at AND NOT numbering)
        OR NEW.finalized_at IS DISTINCT FROM OLD.finalized_at OR NEW.status = 'draft'
    ) THEN
        RAISE EXCEPTION 'finalized invoice is immutable';
    END IF;
    RETURN NEW;
END $$;
