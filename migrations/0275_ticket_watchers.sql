-- Extra staff who follow a ticket and get its notifications.
CREATE TABLE ticket_watchers (
    ticket_id UUID NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    user_id   UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    added_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (ticket_id, user_id)
);
CREATE INDEX ticket_watchers_user_idx ON ticket_watchers (user_id);
