-- GDPR: data-subject erasure requests (export is on demand, nothing stored).
CREATE TABLE gdpr_requests (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID REFERENCES users(id) ON DELETE SET NULL,
    -- Kept after erasure so the request remains auditable.
    subject_ref  TEXT NOT NULL CHECK (length(subject_ref) <= 254),
    status       TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','completed','rejected','cancelled')),
    reason       TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 2000),
    note         TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 2000),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at   TIMESTAMPTZ,
    decided_by   UUID REFERENCES users(id) ON DELETE SET NULL
);
CREATE UNIQUE INDEX gdpr_requests_one_pending ON gdpr_requests (user_id) WHERE status = 'pending';
CREATE INDEX gdpr_requests_status_idx ON gdpr_requests (status, requested_at DESC);

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-000000000103', 'gdpr.manage');
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-000000000103');
