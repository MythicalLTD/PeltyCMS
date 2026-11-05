-- Quotes / proposals. Accepting a sent, unexpired quote turns it into a
-- finalized invoice exactly once (quote row locked, invoice_id set).
CREATE SEQUENCE quote_number_seq;
CREATE TABLE quotes (
    id          UUID PRIMARY KEY,
    number      TEXT NOT NULL UNIQUE,
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    currency    CHAR(3) NOT NULL REFERENCES currencies(code),
    subject     TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 200),
    notes       TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 10000),
    status      TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','sent','accepted','declined')),
    valid_until DATE NOT NULL,
    invoice_id  UUID UNIQUE REFERENCES invoices(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at     TIMESTAMPTZ,
    decided_at  TIMESTAMPTZ
);
CREATE INDEX quotes_customer_idx ON quotes (customer_id, created_at DESC);

CREATE TABLE quote_items (
    id          UUID PRIMARY KEY,
    quote_id    UUID NOT NULL REFERENCES quotes(id) ON DELETE CASCADE,
    description TEXT NOT NULL CHECK (length(description) BETWEEN 1 AND 500),
    quantity    NUMERIC(19,4) NOT NULL CHECK (quantity > 0),
    unit_price  NUMERIC(19,4) NOT NULL,
    tax_rate    NUMERIC(7,4) NOT NULL DEFAULT 0 CHECK (tax_rate >= 0 AND tax_rate <= 100),
    sort_order  INT NOT NULL DEFAULT 0
);
CREATE INDEX quote_items_quote_idx ON quote_items (quote_id, sort_order);

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000a0f02', 'quotes.manage') ON CONFLICT DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id)
  SELECT r.id, p.id FROM roles r, permissions p WHERE r.name = 'admin' AND p.name = 'quotes.manage'
  ON CONFLICT DO NOTHING;
