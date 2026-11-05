-- Support tickets.
CREATE TABLE ticket_departments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    description TEXT NOT NULL DEFAULT '',
    is_active   BOOLEAN NOT NULL DEFAULT true,
    sort_order  INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO ticket_departments (id, name, description, sort_order) VALUES
  ('00000000-0000-0000-0002-000000000001', 'Support', 'Technical help with your services', 0),
  ('00000000-0000-0000-0002-000000000002', 'Billing', 'Invoices, payments and refunds', 1);

CREATE SEQUENCE ticket_number_seq START 1000;
CREATE TABLE tickets (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    number         BIGINT NOT NULL UNIQUE DEFAULT nextval('ticket_number_seq'),
    customer_id    UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    opened_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    department_id  UUID NOT NULL REFERENCES ticket_departments(id) ON DELETE RESTRICT,
    service_id     UUID REFERENCES services(id) ON DELETE SET NULL,
    assigned_to    UUID REFERENCES users(id) ON DELETE SET NULL,
    subject        TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 200),
    status         TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','answered','customer_reply','on_hold','closed')),
    priority       TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('low','medium','high','urgent')),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_reply_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at      TIMESTAMPTZ
);
CREATE INDEX tickets_customer_idx ON tickets (customer_id, last_reply_at DESC);
CREATE INDEX tickets_status_idx ON tickets (status, last_reply_at DESC);

CREATE TABLE ticket_messages (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ticket_id   UUID NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    author_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    is_staff    BOOLEAN NOT NULL,
    is_internal BOOLEAN NOT NULL DEFAULT false CHECK (NOT is_internal OR is_staff),
    body        TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 20000),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ticket_messages_ticket_idx ON ticket_messages (ticket_id, created_at);

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-000000000015', 'tickets.manage');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-000000000015'),
  ('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0001-000000000015');
