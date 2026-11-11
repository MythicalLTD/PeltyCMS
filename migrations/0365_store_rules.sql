-- Game store rules (module `gaming_storefront`): purchase limits, required
-- packages, pay-what-you-want, gifting, and a store ban list.

CREATE TABLE IF NOT EXISTS package_rules (
    product_id           UUID PRIMARY KEY REFERENCES products(id) ON DELETE CASCADE,
    -- Purchases per customer in `user_period` (NULL = no limit).
    user_limit           INT CHECK (user_limit IS NULL OR user_limit BETWEEN 1 AND 100000),
    user_period          TEXT NOT NULL DEFAULT 'all' CHECK (user_period IN ('all', 'day', 'week', 'month')),
    -- Purchases by everyone in `global_period` (NULL = no limit).
    global_limit         INT CHECK (global_limit IS NULL OR global_limit BETWEEN 1 AND 10000000),
    global_period        TEXT NOT NULL DEFAULT 'all' CHECK (global_period IN ('all', 'day', 'week', 'month')),
    -- The buyer must own (or have in the cart) one of these.
    required_product_ids UUID[] NOT NULL DEFAULT '{}',
    -- The listed one-time price is the minimum; buyers may pay more.
    pay_what_you_want    BOOLEAN NOT NULL DEFAULT FALSE,
    giftable             BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- In-game name the line is delivered to when bought as a gift.
ALTER TABLE order_items ADD COLUMN IF NOT EXISTS gift_to TEXT;

CREATE TABLE IF NOT EXISTS store_bans (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- `username` (in-game name) or `email`; value stored lower-case.
    kind       TEXT NOT NULL CHECK (kind IN ('username', 'email')),
    value      TEXT NOT NULL CHECK (length(value) BETWEEN 1 AND 254),
    reason     TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (kind, value)
);
