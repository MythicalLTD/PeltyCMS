-- Creator marketplace (optional `creator_marketplace` module): creators list
-- products for admin approval, earn sales minus commission into a ledger
-- (held for a refund window), and request payouts.
CREATE TABLE creators (
    id                 UUID PRIMARY KEY,
    user_id            UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    customer_id        UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    slug               TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,39}$'),
    display_name       TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 80),
    bio                TEXT NOT NULL DEFAULT '' CHECK (length(bio) <= 5000),
    website            TEXT NOT NULL DEFAULT '' CHECK (length(website) <= 300),
    avatar_url         TEXT NOT NULL DEFAULT '' CHECK (length(avatar_url) <= 500),
    status             TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'suspended', 'rejected')),
    -- NULL = the marketplace default.
    commission_percent NUMERIC(7,4) CHECK (commission_percent IS NULL OR (commission_percent >= 0 AND commission_percent <= 100)),
    payout_method      TEXT NOT NULL DEFAULT 'manual' CHECK (payout_method IN ('manual', 'stripe_connect', 'paypal')),
    payout_details     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    approved_at        TIMESTAMPTZ
);

CREATE TABLE creator_products (
    product_id    UUID PRIMARY KEY REFERENCES products(id) ON DELETE CASCADE,
    creator_id    UUID NOT NULL REFERENCES creators(id) ON DELETE CASCADE,
    status        TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected', 'unlisted')),
    reject_reason TEXT NOT NULL DEFAULT '' CHECK (length(reject_reason) <= 1000),
    submitted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    reviewed_at   TIMESTAMPTZ,
    reviewed_by   UUID REFERENCES users(id) ON DELETE SET NULL
);
CREATE INDEX creator_products_creator ON creator_products (creator_id);

CREATE TABLE creator_ledger (
    id           UUID PRIMARY KEY,
    creator_id   UUID NOT NULL REFERENCES creators(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('sale', 'refund', 'payout', 'payout_reversal', 'adjustment')),
    -- Invoice/order line for sales, the sale row for refunds, the payout for payouts.
    source_id    UUID,
    invoice_id   UUID REFERENCES invoices(id) ON DELETE SET NULL,
    product_id   UUID REFERENCES products(id) ON DELETE SET NULL,
    currency     CHAR(3) NOT NULL REFERENCES currencies(code),
    gross        NUMERIC(19,4) NOT NULL DEFAULT 0,
    commission   NUMERIC(19,4) NOT NULL DEFAULT 0,
    amount       NUMERIC(19,4) NOT NULL,
    available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    note         TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (kind, source_id)
);
CREATE INDEX creator_ledger_creator ON creator_ledger (creator_id, created_at DESC);

CREATE TABLE creator_payouts (
    id           UUID PRIMARY KEY,
    creator_id   UUID NOT NULL REFERENCES creators(id) ON DELETE CASCADE,
    currency     CHAR(3) NOT NULL REFERENCES currencies(code),
    amount       NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    method       TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'requested' CHECK (status IN ('requested', 'processing', 'paid', 'rejected', 'failed')),
    provider_ref TEXT NOT NULL DEFAULT '' CHECK (length(provider_ref) <= 200),
    note         TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 1000),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ
);
CREATE INDEX creator_payouts_creator ON creator_payouts (creator_id, created_at DESC);
