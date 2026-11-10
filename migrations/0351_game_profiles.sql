-- Gaming store (module `game_profiles`): game servers, the packages each
-- server's store sells, and the game identities customers link.
CREATE TABLE game_servers (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    game          TEXT NOT NULL CHECK (game ~ '^[a-z][a-z0-9_]{1,40}$'),
    name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    slug          TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{0,63}$'),
    description   TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    host          TEXT NOT NULL DEFAULT '' CHECK (length(host) <= 255),
    port          INT CHECK (port IS NULL OR port BETWEEN 1 AND 65535),
    query_port    INT CHECK (query_port IS NULL OR query_port BETWEEN 1 AND 65535),
    -- How commands reach the server: rcon (Source RCON: Minecraft, Rust, Source
    -- games), agent (a plugin/resource polls the API: FiveM, RedM, ...) or none.
    transport     TEXT NOT NULL DEFAULT 'none' CHECK (transport IN ('none','rcon','agent')),
    rcon_host     TEXT NOT NULL DEFAULT '' CHECK (length(rcon_host) <= 255),
    rcon_port     INT CHECK (rcon_port IS NULL OR rcon_port BETWEEN 1 AND 65535),
    rcon_password BYTEA,                -- sealed with PELTY_ENCRYPTION_KEY
    agent_token   BYTEA UNIQUE,         -- sha256 of the agent token
    enabled       BOOLEAN NOT NULL DEFAULT true,
    sort_order    INT NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE game_server_products (
    server_id  UUID NOT NULL REFERENCES game_servers(id) ON DELETE CASCADE,
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    sort_order INT NOT NULL DEFAULT 0,
    PRIMARY KEY (server_id, product_id)
);
CREATE INDEX game_server_products_product ON game_server_products (product_id);

CREATE TABLE customer_game_accounts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    game        TEXT NOT NULL,
    field       TEXT NOT NULL CHECK (field ~ '^[a-z][a-z0-9_]{1,30}$'),
    value       TEXT NOT NULL CHECK (length(value) BETWEEN 1 AND 100),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (customer_id, game, field)
);
CREATE INDEX customer_game_accounts_lookup ON customer_game_accounts (game, field, lower(value));
