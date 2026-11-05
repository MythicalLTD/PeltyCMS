-- Email-only guests have no login or customer account.
ALTER TABLE ticket_departments
    ADD COLUMN pipe_unknown_sender TEXT NOT NULL DEFAULT 'reject'
        CHECK (pipe_unknown_sender IN ('reject', 'guest')),
    ADD COLUMN pipe_webhook_secret TEXT NOT NULL DEFAULT '';
ALTER TABLE tickets ALTER COLUMN customer_id DROP NOT NULL;
ALTER TABLE tickets ADD COLUMN guest_email TEXT NOT NULL DEFAULT '';
ALTER TABLE tickets ADD CONSTRAINT tickets_sender_check
    CHECK (customer_id IS NOT NULL OR guest_email <> '');
ALTER TABLE ticket_attachments ALTER COLUMN uploaded_by DROP NOT NULL;
CREATE INDEX ticket_pipe_log_ticket_idx ON ticket_pipe_log (ticket_id) WHERE ticket_id IS NOT NULL;
