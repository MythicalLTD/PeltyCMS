-- In-game delivery (module `game_delivery`): commands per package (or global)
-- run on game servers on purchase, renewal, expiry, refund or chargeback.
CREATE TABLE game_commands (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id     UUID REFERENCES products(id) ON DELETE CASCADE,  -- NULL: global (every purchase)
    server_id      UUID REFERENCES game_servers(id) ON DELETE CASCADE, -- NULL: every server selling it
    trigger        TEXT NOT NULL CHECK (trigger IN ('purchase','renewal','expiry','refund','chargeback')),
    command        TEXT NOT NULL CHECK (length(command) BETWEEN 1 AND 1000 AND command !~ '[\r\n]'),
    require_online BOOLEAN NOT NULL DEFAULT false,
    delay_secs     INT NOT NULL DEFAULT 0 CHECK (delay_secs BETWEEN 0 AND 2592000),
    sort_order     INT NOT NULL DEFAULT 0
);
CREATE INDEX game_commands_product ON game_commands (product_id, trigger);

CREATE TABLE game_deliveries (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    server_id       UUID NOT NULL REFERENCES game_servers(id) ON DELETE CASCADE,
    command_id      UUID REFERENCES game_commands(id) ON DELETE SET NULL,
    customer_id     UUID REFERENCES customers(id) ON DELETE CASCADE,
    service_id      UUID REFERENCES services(id) ON DELETE SET NULL,
    product_id      UUID REFERENCES products(id) ON DELETE SET NULL,
    trigger         TEXT NOT NULL,
    -- Idempotency: one delivery per command, service and occurrence.
    dedupe_key      TEXT NOT NULL UNIQUE,
    template        TEXT NOT NULL,
    command         TEXT NOT NULL DEFAULT '',
    player          TEXT NOT NULL DEFAULT '',
    require_online  BOOLEAN NOT NULL DEFAULT false,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','waiting_identity','delivered','failed','cancelled')),
    attempts        INT NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error      TEXT NOT NULL DEFAULT '',
    response        TEXT NOT NULL DEFAULT '',
    delivered_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX game_deliveries_due ON game_deliveries (server_id, next_attempt_at) WHERE status = 'pending';
CREATE INDEX game_deliveries_customer ON game_deliveries (customer_id, created_at DESC);
CREATE INDEX game_deliveries_waiting ON game_deliveries (customer_id) WHERE status = 'waiting_identity';
