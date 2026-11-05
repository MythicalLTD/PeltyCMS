-- WHMCS "Email Message Log": every transactional mail the worker delivered
-- (or failed to), viewable per client and resendable by staff.
CREATE TABLE email_log (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID REFERENCES users(id) ON DELETE SET NULL,
    recipient   TEXT NOT NULL,
    template    TEXT NOT NULL,
    locale      TEXT NOT NULL,
    subject     TEXT NOT NULL,
    body_text   TEXT NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('sent', 'failed')),
    error       TEXT,
    mailer      TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX email_log_user_idx ON email_log (user_id, created_at DESC);
CREATE INDEX email_log_created_idx ON email_log (created_at DESC);

-- Overdue automation gains termination and invoice auto-cancellation.
ALTER TABLE overdue_actions DROP CONSTRAINT overdue_actions_action_check;
ALTER TABLE overdue_actions ADD CONSTRAINT overdue_actions_action_check
    CHECK (action ~ '^(reminder:[0-9]{1,3}|late_fee|suspend|terminate|auto_void)$');
