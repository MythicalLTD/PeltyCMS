-- Custom ticket statuses. Built-in ones keep their workflow meaning
-- (replies move tickets between open/answered/customer_reply/closed);
-- custom ones are set manually by staff.
CREATE TABLE ticket_statuses (
    slug        TEXT PRIMARY KEY CHECK (slug ~ '^[a-z][a-z0-9_]{1,31}$'),
    name        TEXT NOT NULL DEFAULT '',
    color       TEXT NOT NULL DEFAULT 'secondary' CHECK (color IN ('default','secondary','destructive','outline')),
    sort_order  INT NOT NULL DEFAULT 0,
    awaiting_staff BOOLEAN NOT NULL DEFAULT FALSE,
    builtin     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO ticket_statuses (slug, color, sort_order, awaiting_staff, builtin) VALUES
    ('open', 'secondary', 10, TRUE, TRUE),
    ('answered', 'default', 20, FALSE, TRUE),
    ('customer_reply', 'secondary', 30, TRUE, TRUE),
    ('on_hold', 'secondary', 40, FALSE, TRUE),
    ('closed', 'outline', 100, FALSE, TRUE);

ALTER TABLE tickets DROP CONSTRAINT IF EXISTS tickets_status_check;
ALTER TABLE tickets ADD CONSTRAINT tickets_status_fkey FOREIGN KEY (status) REFERENCES ticket_statuses(slug);
