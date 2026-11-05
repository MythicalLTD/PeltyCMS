-- Overage billing from server-module usage + server account import.
ALTER TABLE usage_metrics
    ADD COLUMN source_key TEXT NOT NULL DEFAULT '' CHECK (source_key ~ '^[A-Za-z0-9_.]{0,80}$'),
    ADD COLUMN source_divisor NUMERIC(24,6) NOT NULL DEFAULT 1 CHECK (source_divisor > 0),
    ADD COLUMN aggregate TEXT NOT NULL DEFAULT 'sum' CHECK (aggregate IN ('sum','max'));

ALTER TABLE services ADD COLUMN imported_at TIMESTAMPTZ;
