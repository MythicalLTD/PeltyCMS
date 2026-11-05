-- Mass mail campaigns to client segments, sent by the job queue.
ALTER TABLE customers ADD COLUMN IF NOT EXISTS marketing_opt_out BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE mail_campaigns (
    id           UUID PRIMARY KEY,
    name         TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 200),
    subject      TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 300),
    body         TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 50000),
    segment      JSONB NOT NULL DEFAULT '{}'::jsonb,
    status       TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','sending','sent','cancelled')),
    created_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    queued_at    TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ
);

CREATE TABLE mail_campaign_recipients (
    campaign_id  UUID NOT NULL REFERENCES mail_campaigns(id) ON DELETE CASCADE,
    customer_id  UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    email        CITEXT NOT NULL,
    unsub_token  UUID NOT NULL UNIQUE,
    status       TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','sent','failed','skipped')),
    error        TEXT,
    sent_at      TIMESTAMPTZ,
    PRIMARY KEY (campaign_id, customer_id)
);
CREATE INDEX mail_campaign_recipients_pending ON mail_campaign_recipients (campaign_id) WHERE status = 'pending';

INSERT INTO permissions (id, name) VALUES ('00000000-0000-0000-0001-0000000f1005', 'campaigns.manage') ON CONFLICT DO NOTHING;
INSERT INTO role_permissions (role_id, permission_id) VALUES
  ('00000000-0000-0000-0000-000000000001', '00000000-0000-0000-0001-0000000f1005') ON CONFLICT DO NOTHING;
