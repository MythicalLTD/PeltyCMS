-- Ticket SLA policies per department and priority: first-response and
-- resolution targets, a warning threshold and what to do on a breach.
-- The most specific policy wins (department+priority, department, priority,
-- then the catch-all with both NULL).
CREATE TABLE ticket_sla_policies (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    department_id          UUID REFERENCES ticket_departments(id) ON DELETE CASCADE,
    priority               TEXT CHECK (priority IN ('low','medium','high','urgent')),
    first_response_minutes INT CHECK (first_response_minutes BETWEEN 1 AND 525600),
    resolution_minutes     INT CHECK (resolution_minutes BETWEEN 1 AND 525600),
    warn_percent           SMALLINT NOT NULL DEFAULT 80 CHECK (warn_percent BETWEEN 1 AND 99),
    escalate_priority      TEXT CHECK (escalate_priority IN ('low','medium','high','urgent')),
    escalate_to            UUID REFERENCES users(id) ON DELETE SET NULL,
    escalate_department    UUID REFERENCES ticket_departments(id) ON DELETE SET NULL,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (first_response_minutes IS NOT NULL OR resolution_minutes IS NOT NULL)
);
CREATE UNIQUE INDEX ticket_sla_policies_scope
    ON ticket_sla_policies (COALESCE(department_id, '00000000-0000-0000-0000-000000000000'::uuid), COALESCE(priority, ''));

-- Warnings and breaches fire once per ticket and target.
CREATE TABLE ticket_sla_events (
    ticket_id UUID NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    target    TEXT NOT NULL CHECK (target IN ('first_response','resolution')),
    stage     TEXT NOT NULL CHECK (stage IN ('warning','breach')),
    policy_id UUID REFERENCES ticket_sla_policies(id) ON DELETE SET NULL,
    due_at    TIMESTAMPTZ NOT NULL,
    at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (ticket_id, target, stage)
);
CREATE INDEX ticket_sla_events_at ON ticket_sla_events (at DESC);

-- Department first-response SLAs (migration 0120) become policies.
INSERT INTO ticket_sla_policies (department_id, first_response_minutes)
SELECT id, sla_first_response_minutes FROM ticket_departments WHERE sla_first_response_minutes IS NOT NULL;
