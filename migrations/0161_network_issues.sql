-- Network / service status issues (WHMCS "Network Issues"), public page + admin.
CREATE TABLE network_issues (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title       TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 20000),
    component   TEXT NOT NULL DEFAULT '' CHECK (length(component) <= 120),   -- e.g. "EU-1 nodes"
    kind        TEXT NOT NULL DEFAULT 'outage' CHECK (kind IN ('outage','degraded','maintenance')),
    priority    TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('low','medium','high','critical')),
    status      TEXT NOT NULL DEFAULT 'investigating'
                CHECK (status IN ('scheduled','investigating','identified','monitoring','resolved')),
    starts_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    ends_at     TIMESTAMPTZ,
    created_by  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (ends_at IS NULL OR ends_at >= starts_at)
);
CREATE INDEX network_issues_open ON network_issues (starts_at DESC) WHERE status <> 'resolved';
CREATE INDEX network_issues_recent ON network_issues (created_at DESC);

CREATE TABLE network_issue_updates (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    issue_id   UUID NOT NULL REFERENCES network_issues(id) ON DELETE CASCADE,
    status     TEXT NOT NULL CHECK (status IN ('scheduled','investigating','identified','monitoring','resolved')),
    message    TEXT NOT NULL CHECK (length(message) BETWEEN 1 AND 5000),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX network_issue_updates_issue ON network_issue_updates (issue_id, created_at);

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000f1007', 'status.manage');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-0000000f1007');
