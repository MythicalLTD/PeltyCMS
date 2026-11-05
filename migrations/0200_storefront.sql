-- Public storefront carts (keyed by an unguessable client token), customer
-- contacts / sub-accounts, email preferences and admin order notes.
CREATE TABLE carts (
    id           UUID PRIMARY KEY,
    customer_id  UUID REFERENCES customers(id) ON DELETE CASCADE,
    email        TEXT,
    currency     CHAR(3) NOT NULL REFERENCES currencies(code),
    country      TEXT NOT NULL DEFAULT '' CHECK (country = '' OR country ~ '^[A-Z]{2}$'),
    items        JSONB NOT NULL DEFAULT '[]'::jsonb CHECK (jsonb_typeof(items) = 'array'),
    coupon       TEXT,
    locale       TEXT NOT NULL DEFAULT '' CHECK (length(locale) <= 16),
    order_id     UUID REFERENCES orders(id) ON DELETE SET NULL,
    reminded_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX carts_abandoned_idx ON carts (updated_at) WHERE order_id IS NULL AND reminded_at IS NULL;

CREATE TABLE customer_subaccounts (
    id           UUID PRIMARY KEY,
    customer_id  UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    user_id      UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL DEFAULT '',
    permissions  TEXT[] NOT NULL DEFAULT '{}'
        CHECK (permissions <@ ARRAY['invoices','tickets','services']::text[]),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX customer_subaccounts_customer_idx ON customer_subaccounts (customer_id);

CREATE TABLE customer_email_prefs (
    customer_id  UUID PRIMARY KEY REFERENCES customers(id) ON DELETE CASCADE,
    invoices     BOOLEAN NOT NULL DEFAULT true,
    services     BOOLEAN NOT NULL DEFAULT true,
    support      BOOLEAN NOT NULL DEFAULT true,
    marketing    BOOLEAN NOT NULL DEFAULT true,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE order_notes (
    id          UUID PRIMARY KEY,
    order_id    UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    author_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    body        TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 5000),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX order_notes_order_idx ON order_notes (order_id, created_at);
