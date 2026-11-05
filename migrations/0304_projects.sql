-- Project management (WHMCS Project Management addon): projects linked to a
-- client, tickets and invoices, with tasks and time tracking billed as
-- invoice lines.
CREATE TABLE projects (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    customer_id UUID REFERENCES customers(id) ON DELETE SET NULL,
    title       TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 10000),
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('planned','active','on_hold','completed','cancelled')),
    due_date    DATE,
    -- Visible to the client in the client area.
    client_visible BOOLEAN NOT NULL DEFAULT false,
    -- Hourly rate for billable time (in the client's currency).
    hourly_rate NUMERIC(14,2) NOT NULL DEFAULT 0 CHECK (hourly_rate >= 0),
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX projects_customer_idx ON projects (customer_id, created_at DESC);

CREATE TABLE project_links (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN ('ticket','invoice')),
    target_id  UUID NOT NULL,
    PRIMARY KEY (project_id, kind, target_id)
);

CREATE TABLE project_tasks (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title       TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 300),
    done        BOOLEAN NOT NULL DEFAULT false,
    assignee_id UUID REFERENCES users(id) ON DELETE SET NULL,
    due_date    DATE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX project_tasks_project_idx ON project_tasks (project_id, created_at);

CREATE TABLE project_time_entries (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id     UUID REFERENCES project_tasks(id) ON DELETE SET NULL,
    user_id     UUID REFERENCES users(id) ON DELETE SET NULL,
    minutes     INT NOT NULL CHECK (minutes BETWEEN 1 AND 1440),
    note        TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    billable    BOOLEAN NOT NULL DEFAULT true,
    spent_on    DATE NOT NULL DEFAULT CURRENT_DATE,
    invoice_id  UUID REFERENCES invoices(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX project_time_project_idx ON project_time_entries (project_id, spent_on DESC);
