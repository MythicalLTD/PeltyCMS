-- Support suite: canned replies, tags, ratings, SLA first response,
-- merge, escalation rules and inbound e-mail piping.
ALTER TABLE tickets
    ADD COLUMN first_response_at TIMESTAMPTZ,
    ADD COLUMN rating SMALLINT CHECK (rating BETWEEN 1 AND 5),
    ADD COLUMN rating_comment TEXT NOT NULL DEFAULT '' CHECK (length(rating_comment) <= 2000),
    ADD COLUMN merged_into UUID REFERENCES tickets(id) ON DELETE SET NULL,
    ADD COLUMN escalated_at TIMESTAMPTZ;

-- Backfill first response from existing staff replies.
UPDATE tickets t SET first_response_at = m.at
FROM (SELECT ticket_id, min(created_at) AS at FROM ticket_messages WHERE is_staff AND NOT is_internal GROUP BY ticket_id) m
WHERE m.ticket_id = t.id;

ALTER TABLE ticket_departments
    ADD COLUMN email TEXT NOT NULL DEFAULT '' CHECK (length(email) <= 254),
    ADD COLUMN sla_first_response_minutes INT CHECK (sla_first_response_minutes > 0);

CREATE TABLE ticket_canned_replies (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title         TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 150),
    body          TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 20000),
    department_id UUID REFERENCES ticket_departments(id) ON DELETE CASCADE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE ticket_tags (
    ticket_id UUID NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    tag       TEXT NOT NULL CHECK (tag ~ '^[a-z0-9][a-z0-9_-]{0,39}$'),
    PRIMARY KEY (ticket_id, tag)
);
CREATE INDEX ticket_tags_tag_idx ON ticket_tags (tag);

CREATE TABLE ticket_escalation_rules (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name           TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    department_id  UUID REFERENCES ticket_departments(id) ON DELETE CASCADE,
    statuses       TEXT[] NOT NULL DEFAULT ARRAY['open','customer_reply'],
    priority       TEXT CHECK (priority IN ('low','medium','high','urgent')),
    after_minutes  INT NOT NULL CHECK (after_minutes > 0),
    set_priority   TEXT CHECK (set_priority IN ('low','medium','high','urgent')),
    set_department UUID REFERENCES ticket_departments(id) ON DELETE SET NULL,
    assign_to      UUID REFERENCES users(id) ON DELETE SET NULL,
    add_tag        TEXT CHECK (add_tag ~ '^[a-z0-9][a-z0-9_-]{0,39}$'),
    is_active      BOOLEAN NOT NULL DEFAULT true,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A rule fires at most once per ticket.
CREATE TABLE ticket_escalations (
    rule_id   UUID NOT NULL REFERENCES ticket_escalation_rules(id) ON DELETE CASCADE,
    ticket_id UUID NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    fired_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (rule_id, ticket_id)
);

-- Inbound mail dedupe (Message-ID) and outcome log.
CREATE TABLE ticket_pipe_log (
    message_id TEXT PRIMARY KEY CHECK (length(message_id) BETWEEN 1 AND 998),
    sender     TEXT NOT NULL,
    subject    TEXT NOT NULL,
    outcome    TEXT NOT NULL CHECK (outcome IN ('opened','replied','rejected')),
    ticket_id  UUID REFERENCES tickets(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
