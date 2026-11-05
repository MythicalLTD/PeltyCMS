-- Legal entities (selling companies): per-entity bank details, footer/terms,
-- due-date terms and per-year gapless numbering. Without any entity the
-- legacy global INV-/CN- sequence is used unchanged.
CREATE TABLE legal_entities (
    id             UUID PRIMARY KEY,
    name           TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    address        TEXT NOT NULL DEFAULT '',
    tax_id         TEXT NOT NULL DEFAULT '',
    bank_name      TEXT NOT NULL DEFAULT '',
    iban           TEXT NOT NULL DEFAULT '',
    swift          TEXT NOT NULL DEFAULT '',
    footer         TEXT NOT NULL DEFAULT '',
    terms          TEXT NOT NULL DEFAULT '',
    invoice_prefix TEXT NOT NULL,
    credit_prefix  TEXT NOT NULL,
    number_format  TEXT NOT NULL DEFAULT '{prefix}-{year}-{seq:5}',
    due_days       INT CHECK (due_days BETWEEN 0 AND 365),
    is_default     BOOLEAN NOT NULL DEFAULT false,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX legal_entities_one_default ON legal_entities (is_default) WHERE is_default;
CREATE UNIQUE INDEX legal_entities_invoice_prefix ON legal_entities (lower(invoice_prefix));
CREATE UNIQUE INDEX legal_entities_credit_prefix ON legal_entities (lower(credit_prefix));

CREATE TABLE legal_entity_sequences (
    entity_id UUID NOT NULL REFERENCES legal_entities(id) ON DELETE RESTRICT,
    kind      TEXT NOT NULL CHECK (kind IN ('invoice', 'credit_note')),
    year      INT NOT NULL,
    next      BIGINT NOT NULL CHECK (next > 0),
    PRIMARY KEY (entity_id, kind, year)
);

ALTER TABLE customers ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id) ON DELETE SET NULL;
-- Frozen at finalization so later edits never change issued documents.
ALTER TABLE invoices ADD COLUMN legal_entity_id UUID REFERENCES legal_entities(id) ON DELETE RESTRICT,
                     ADD COLUMN seller_snapshot JSONB;
