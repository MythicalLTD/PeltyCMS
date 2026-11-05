-- WHMCS-style link tracking: short redirect links that count clicks.
CREATE TABLE tracked_links (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}$'),
    name TEXT NOT NULL,
    target_url TEXT NOT NULL CHECK (target_url ~ '^https?://'),
    clicks BIGINT NOT NULL DEFAULT 0,
    last_click_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
