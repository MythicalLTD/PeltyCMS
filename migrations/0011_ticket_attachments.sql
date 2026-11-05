-- Ticket attachments. Uploaded first (pending, owned by the uploader), then
-- linked to a message when the reply/ticket is posted. Bytes live in the DB
-- so there is no filesystem path handling; served only as downloads.
CREATE TABLE ticket_attachments (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    uploaded_by  UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    ticket_id    UUID REFERENCES tickets(id) ON DELETE CASCADE,
    message_id   UUID REFERENCES ticket_messages(id) ON DELETE CASCADE,
    filename     TEXT NOT NULL CHECK (length(filename) BETWEEN 1 AND 200),
    content_type TEXT NOT NULL,
    size_bytes   INT NOT NULL CHECK (size_bytes > 0),
    data         BYTEA NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((ticket_id IS NULL) = (message_id IS NULL))
);
CREATE INDEX ticket_attachments_message_idx ON ticket_attachments (message_id);
CREATE INDEX ticket_attachments_pending_idx ON ticket_attachments (uploaded_by, created_at) WHERE message_id IS NULL;
