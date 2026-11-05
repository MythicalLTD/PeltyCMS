-- Ticket macros: one click posts an optional reply or note and sets status,
-- priority, assignee, department and tags.
CREATE TABLE ticket_macros (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name           TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    description    TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 500),
    -- NULL: offered in every department.
    department_id  UUID REFERENCES ticket_departments(id) ON DELETE CASCADE,
    reply_body     TEXT NOT NULL DEFAULT '' CHECK (length(reply_body) <= 20000),
    reply_internal BOOLEAN NOT NULL DEFAULT false,
    set_status     TEXT,
    set_priority   TEXT CHECK (set_priority IN ('low','medium','high','urgent')),
    -- keep | self (whoever runs it) | user (assign_to) | unassign
    assign_mode    TEXT NOT NULL DEFAULT 'keep' CHECK (assign_mode IN ('keep','self','user','unassign')),
    assign_to      UUID REFERENCES users(id) ON DELETE SET NULL,
    set_department UUID REFERENCES ticket_departments(id) ON DELETE SET NULL,
    add_tags       TEXT[] NOT NULL DEFAULT '{}',
    is_active      BOOLEAN NOT NULL DEFAULT true,
    sort_order     INT NOT NULL DEFAULT 0,
    uses           BIGINT NOT NULL DEFAULT 0,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (assign_mode <> 'user' OR assign_to IS NOT NULL)
);
CREATE INDEX ticket_macros_department ON ticket_macros (department_id);
