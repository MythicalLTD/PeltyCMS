-- Server community (optional `server_community` module): cached server
-- status, vote sites with HTTP vote callbacks and rewards, and votes.
CREATE TABLE game_server_status (
    server_id   UUID PRIMARY KEY REFERENCES game_servers(id) ON DELETE CASCADE,
    online      BOOLEAN NOT NULL DEFAULT false,
    players     INT NOT NULL DEFAULT 0,
    max_players INT NOT NULL DEFAULT 0,
    version     TEXT NOT NULL DEFAULT '',
    motd        TEXT NOT NULL DEFAULT '',
    sample      TEXT[] NOT NULL DEFAULT '{}',
    latency_ms  INT,
    error       TEXT NOT NULL DEFAULT '',
    peak_players INT NOT NULL DEFAULT 0,
    checked_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE vote_sites (
    id             UUID PRIMARY KEY,
    name           TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    -- Link shown to players; `{username}` is replaced with their name.
    url            TEXT NOT NULL CHECK (url ~ '^https?://' AND length(url) <= 500),
    token_hash     BYTEA,
    server_id      UUID REFERENCES game_servers(id) ON DELETE SET NULL,
    reward_command TEXT NOT NULL DEFAULT '' CHECK (length(reward_command) <= 1000),
    require_online BOOLEAN NOT NULL DEFAULT false,
    cooldown_hours INT NOT NULL DEFAULT 24 CHECK (cooldown_hours BETWEEN 0 AND 168),
    enabled        BOOLEAN NOT NULL DEFAULT true,
    sort_order     INT NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE votes (
    id          UUID PRIMARY KEY,
    site_id     UUID NOT NULL REFERENCES vote_sites(id) ON DELETE CASCADE,
    username    TEXT NOT NULL CHECK (length(username) BETWEEN 1 AND 64),
    customer_id UUID REFERENCES customers(id) ON DELETE SET NULL,
    ip          TEXT NOT NULL DEFAULT '',
    rewarded    INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX votes_site_user ON votes (site_id, lower(username), created_at DESC);
CREATE INDEX votes_created ON votes (created_at DESC);
