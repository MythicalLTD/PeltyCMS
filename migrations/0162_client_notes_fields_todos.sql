-- Staff notes on clients (multiple, sticky), custom client fields and the
-- admin to-do list.
CREATE TABLE customer_notes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    author_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    body        TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 10000),
    sticky      BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX customer_notes_customer ON customer_notes (customer_id, sticky DESC, created_at DESC);

CREATE TABLE client_fields (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 500),
    field_type  TEXT NOT NULL CHECK (field_type IN ('text','textarea','dropdown','checkbox','link')),
    options     TEXT[] NOT NULL DEFAULT '{}',
    required    BOOLEAN NOT NULL DEFAULT false,
    admin_only  BOOLEAN NOT NULL DEFAULT false,
    sort_order  INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE customer_field_values (
    customer_id UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    field_id    UUID NOT NULL REFERENCES client_fields(id) ON DELETE CASCADE,
    value       TEXT NOT NULL CHECK (length(value) <= 2000),
    PRIMARY KEY (customer_id, field_id)
);

CREATE TABLE admin_todos (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title       TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 5000),
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','in_progress','done')),
    due_date    DATE,
    assigned_to UUID REFERENCES users(id) ON DELETE SET NULL,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX admin_todos_open ON admin_todos (status, due_date);

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000f1004', 'todos.manage');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-0000000f1004'),
  ('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0001-0000000f1004');
