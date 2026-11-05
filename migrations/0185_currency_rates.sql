-- Exchange rates (relative to the default currency, which has rate 1),
-- display prefix/suffix and a single default currency.
ALTER TABLE currencies
    ADD COLUMN rate       NUMERIC(20,10) NOT NULL DEFAULT 1 CHECK (rate > 0),
    ADD COLUMN is_default BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN prefix     TEXT NOT NULL DEFAULT '' CHECK (length(prefix) <= 8),
    ADD COLUMN suffix     TEXT NOT NULL DEFAULT '' CHECK (length(suffix) <= 8),
    ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
UPDATE currencies SET is_default = TRUE WHERE code = 'EUR';
CREATE UNIQUE INDEX currencies_one_default ON currencies (is_default) WHERE is_default;
