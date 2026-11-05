-- Server groups: products bound to a group get a server picked automatically
-- (per module, enabled, under max_services) by the group's strategy.
CREATE TABLE server_groups (
    id         UUID PRIMARY KEY,
    name       TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    strategy   TEXT NOT NULL DEFAULT 'least_services' CHECK (strategy IN ('least_services','fill','round_robin')),
    rr_cursor  BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE servers ADD COLUMN group_id UUID REFERENCES server_groups(id) ON DELETE SET NULL;
ALTER TABLE products ADD COLUMN server_group_id UUID REFERENCES server_groups(id) ON DELETE SET NULL;
CREATE INDEX servers_group_idx ON servers (group_id) WHERE group_id IS NOT NULL;
