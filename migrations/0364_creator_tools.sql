-- Creator tools (module `creator_marketplace`): creator-owned discount codes
-- and bundles, wishlists, buyer <-> creator conversations, listing views.

-- Discount codes a creator made; always limited to that creator's listings.
ALTER TABLE coupons ADD COLUMN IF NOT EXISTS creator_id UUID REFERENCES creators(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS coupons_creator ON coupons (creator_id) WHERE creator_id IS NOT NULL;

-- Bundles a creator made from their own listings.
ALTER TABLE bundles ADD COLUMN IF NOT EXISTS creator_id UUID REFERENCES creators(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS bundles_creator ON bundles (creator_id) WHERE creator_id IS NOT NULL;

-- Bundle discount on each line, so creator earnings use what the buyer paid.
ALTER TABLE order_items ADD COLUMN IF NOT EXISTS bundle_discount NUMERIC(19,4) NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS wishlist_items (
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    product_id  UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (customer_id, product_id)
);
CREATE INDEX IF NOT EXISTS wishlist_items_product ON wishlist_items (product_id);

CREATE TABLE IF NOT EXISTS market_conversations (
    id              UUID PRIMARY KEY,
    creator_id      UUID NOT NULL REFERENCES creators(id) ON DELETE CASCADE,
    customer_id     UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    product_id      UUID REFERENCES products(id) ON DELETE SET NULL,
    -- Latest paid invoice for the product when the conversation started.
    invoice_id      UUID REFERENCES invoices(id) ON DELETE SET NULL,
    subject         TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 150),
    status          TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed')),
    buyer_unread    INT NOT NULL DEFAULT 0,
    creator_unread  INT NOT NULL DEFAULT 0,
    last_message_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS market_conversations_creator ON market_conversations (creator_id, last_message_at DESC);
CREATE INDEX IF NOT EXISTS market_conversations_customer ON market_conversations (customer_id, last_message_at DESC);

CREATE TABLE IF NOT EXISTS market_messages (
    id              UUID PRIMARY KEY,
    conversation_id UUID NOT NULL REFERENCES market_conversations(id) ON DELETE CASCADE,
    author_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    from_creator    BOOLEAN NOT NULL,
    body            TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 5000),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS market_messages_conversation ON market_messages (conversation_id, created_at);

-- Resource page views per day (public marketplace).
CREATE TABLE IF NOT EXISTS product_views (
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    day        DATE NOT NULL,
    views      INT NOT NULL DEFAULT 0,
    PRIMARY KEY (product_id, day)
);
