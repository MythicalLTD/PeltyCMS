-- Discord roles (module `discord_roles`): give buyers Discord server roles
-- for products and take them back on expiry, refund or chargeback. The bot
-- token is sealed with the encryption key; buyers are matched through their
-- linked Discord login (`user_identities`, provider `discord`).
CREATE TABLE discord_role_config (
    id            BOOLEAN PRIMARY KEY DEFAULT true CHECK (id),
    guild_id      TEXT NOT NULL DEFAULT '' CHECK (guild_id = '' OR guild_id ~ '^[0-9]{17,20}$'),
    bot_token_enc BYTEA,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE discord_role_rules (
    product_id       UUID PRIMARY KEY REFERENCES products(id) ON DELETE CASCADE,
    role_ids         TEXT[] NOT NULL CHECK (cardinality(role_ids) BETWEEN 1 AND 10),
    remove_on_expiry BOOLEAN NOT NULL DEFAULT true,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE discord_role_jobs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    service_id  UUID REFERENCES services(id) ON DELETE SET NULL,
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id     TEXT NOT NULL CHECK (role_id ~ '^[0-9]{17,20}$'),
    action      TEXT NOT NULL CHECK (action IN ('add','remove')),
    occurrence  TEXT NOT NULL CHECK (length(occurrence) <= 120),
    status      TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending','waiting','done','skipped','failed','cancelled')),
    attempts    INT NOT NULL DEFAULT 0,
    last_error  TEXT NOT NULL DEFAULT '' CHECK (length(last_error) <= 500),
    next_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    done_at     TIMESTAMPTZ,
    UNIQUE (service_id, role_id, action, occurrence)
);
CREATE INDEX discord_role_jobs_due ON discord_role_jobs (next_at) WHERE status IN ('pending','waiting');
CREATE INDEX discord_role_jobs_user ON discord_role_jobs (user_id, created_at DESC);
