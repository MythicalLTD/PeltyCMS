-- When a customer rated a ticket (satisfaction report period filter).
ALTER TABLE tickets ADD COLUMN rated_at TIMESTAMPTZ;
UPDATE tickets SET rated_at = COALESCE(closed_at, last_reply_at) WHERE rating IS NOT NULL;
CREATE INDEX tickets_rated_at ON tickets (rated_at) WHERE rating IS NOT NULL;
