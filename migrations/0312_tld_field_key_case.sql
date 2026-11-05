-- Registrar parameter names are case-sensitive (RegistrantNexus, X-CA-LEGALTYPE,
-- contactRegistrant.nexusCategory): keep the key exactly as typed, but two keys
-- differing only in case are still one field.
ALTER TABLE tld_fields DROP CONSTRAINT IF EXISTS tld_fields_key_check;
ALTER TABLE tld_fields ADD CONSTRAINT tld_fields_key_check CHECK (key ~ '^[A-Za-z][A-Za-z0-9_.-]{0,63}$');
CREATE UNIQUE INDEX IF NOT EXISTS tld_fields_tld_key_ci ON tld_fields (tld, lower(key));
