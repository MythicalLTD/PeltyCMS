-- Tier upgrades (module `tier_upgrades`): packages in a tier group (VIP <
-- VIP+ < MVP). A player who owns a lower tier pays the higher one minus a
-- credit: what they actually paid for the tier (cumulative), or the list
-- price difference.
CREATE TABLE tier_groups (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                 TEXT NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 100),
    credit_mode          TEXT NOT NULL DEFAULT 'paid' CHECK (credit_mode IN ('paid','difference')),
    -- Lowest price an upgrade can cost, in the default currency (converted).
    min_price            NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (min_price >= 0 AND min_price <= 100000),
    -- Run the old tier's `expiry` commands when the upgrade is delivered.
    revoke_old           BOOLEAN NOT NULL DEFAULT false,
    -- Cancel a recurring lower tier so it stops billing.
    cancel_old_recurring BOOLEAN NOT NULL DEFAULT true,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tier_group_items (
    group_id   UUID NOT NULL REFERENCES tier_groups(id) ON DELETE CASCADE,
    -- A package belongs to at most one tier group.
    product_id UUID PRIMARY KEY REFERENCES products(id) ON DELETE CASCADE,
    rank       INT NOT NULL CHECK (rank BETWEEN 1 AND 100),
    UNIQUE (group_id, rank)
);

-- What an order line was charged before tax after every discount (first
-- invoice), and the upgrade credit it received.
ALTER TABLE order_items
    ADD COLUMN net_amount     NUMERIC(19,4),
    ADD COLUMN upgrade_credit NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK (upgrade_credit >= 0);

-- Lower tiers an upgrade line replaces; `source` is the one whose value was
-- credited (never more than one per line, so credit is never counted twice).
CREATE TABLE tier_upgrades (
    order_item_id   UUID NOT NULL REFERENCES order_items(id) ON DELETE CASCADE,
    from_service_id UUID NOT NULL REFERENCES services(id) ON DELETE CASCADE,
    source          BOOLEAN NOT NULL DEFAULT false,
    credit          NUMERIC(19,4) NOT NULL DEFAULT 0,
    -- Set when the upgrade cancelled this (recurring) lower tier.
    cancelled_at    TIMESTAMPTZ,
    PRIMARY KEY (order_item_id, from_service_id)
);
CREATE UNIQUE INDEX tier_upgrades_one_source ON tier_upgrades (order_item_id) WHERE source;
CREATE INDEX tier_upgrades_from ON tier_upgrades (from_service_id);
