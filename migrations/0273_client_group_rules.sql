-- Automatic client-group assignment rules. Evaluated in priority order; the
-- first matching rule decides the group. Applied on demand by an admin
-- (dry-run first) and only to customers without a group unless overwrite.
CREATE TABLE client_group_rules (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    group_id   UUID NOT NULL REFERENCES client_groups(id) ON DELETE CASCADE,
    priority   INT  NOT NULL DEFAULT 100 CHECK (priority BETWEEN 0 AND 10000),
    kind       TEXT NOT NULL CHECK (kind IN ('country','email_domain','active_services_min','account_age_days_min')),
    -- country / email_domain: comma-separated list; *_min: non-negative integer.
    value      TEXT NOT NULL CHECK (length(value) BETWEEN 1 AND 1000),
    enabled    BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX client_group_rules_order ON client_group_rules (priority, created_at) WHERE enabled;
