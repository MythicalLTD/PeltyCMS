-- Support extensions (WHMCS parity): department auto-reply and mailbox
-- piping (IMAP/POP3), spam control, canned categories, time tracking,
-- KB tags and contact form messages.
ALTER TABLE ticket_departments
    ADD COLUMN auto_reply      BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN auto_reply_text TEXT NOT NULL DEFAULT '' CHECK (length(auto_reply_text) <= 5000),
    ADD COLUMN pipe_protocol   TEXT NOT NULL DEFAULT 'none' CHECK (pipe_protocol IN ('none','imap','pop3')),
    ADD COLUMN pipe_host       TEXT NOT NULL DEFAULT '' CHECK (length(pipe_host) <= 253),
    ADD COLUMN pipe_port       INT NOT NULL DEFAULT 993 CHECK (pipe_port BETWEEN 1 AND 65535),
    ADD COLUMN pipe_tls        BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN pipe_username   TEXT NOT NULL DEFAULT '' CHECK (length(pipe_username) <= 254),
    ADD COLUMN pipe_password   TEXT NOT NULL DEFAULT '',
    ADD COLUMN pipe_last_run   TIMESTAMPTZ,
    ADD COLUMN pipe_last_error TEXT NOT NULL DEFAULT '';

ALTER TABLE ticket_pipe_log DROP CONSTRAINT IF EXISTS ticket_pipe_log_outcome_check;
ALTER TABLE ticket_pipe_log ADD CONSTRAINT ticket_pipe_log_outcome_check CHECK (outcome IN ('opened','replied','rejected','spam'));

CREATE TABLE ticket_spam_rules (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    kind       TEXT NOT NULL CHECK (kind IN ('sender','domain','subject','phrase')),
    value      TEXT NOT NULL CHECK (length(value) BETWEEN 1 AND 254),
    hits       BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (kind, value)
);

ALTER TABLE ticket_canned_replies
    ADD COLUMN category TEXT NOT NULL DEFAULT '' CHECK (length(category) <= 100);

CREATE TABLE ticket_time_entries (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ticket_id  UUID NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    staff_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    minutes    INT NOT NULL CHECK (minutes BETWEEN 1 AND 1440),
    note       TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 500),
    billable   BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ticket_time_entries_ticket_idx ON ticket_time_entries (ticket_id, created_at);

ALTER TABLE kb_articles ADD COLUMN tags TEXT[] NOT NULL DEFAULT '{}';
CREATE INDEX kb_articles_tags_idx ON kb_articles USING gin (tags);

CREATE TABLE contact_messages (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    email      TEXT NOT NULL CHECK (length(email) BETWEEN 3 AND 254),
    subject    TEXT NOT NULL CHECK (length(subject) BETWEEN 1 AND 200),
    body       TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 10000),
    ip         TEXT,
    status     TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new','read','spam','converted')),
    ticket_id  UUID REFERENCES tickets(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX contact_messages_created_idx ON contact_messages (created_at DESC);

-- Automation settings: per-task enable flag and preferred UTC start hour.
CREATE TABLE automation_settings (
    kind TEXT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    run_hour SMALLINT CHECK (run_hour BETWEEN 0 AND 23),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
