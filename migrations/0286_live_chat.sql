-- Simple live chat: anonymous visitor sessions (bearer token, hashed) and
-- staff replies, polled by both sides.
CREATE TABLE chat_sessions (
    id              UUID PRIMARY KEY,
    token_hash      BYTEA NOT NULL UNIQUE,
    customer_id     UUID REFERENCES customers(id) ON DELETE SET NULL,
    name            TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    email           TEXT NOT NULL CHECK (length(email) BETWEEN 3 AND 254),
    page            TEXT NOT NULL DEFAULT '' CHECK (length(page) <= 500),
    status          TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','closed')),
    assigned_to     UUID REFERENCES users(id) ON DELETE SET NULL,
    staff_unread    INT NOT NULL DEFAULT 0,
    visitor_unread  INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_message_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at       TIMESTAMPTZ
);
CREATE INDEX chat_sessions_open_idx ON chat_sessions (last_message_at DESC) WHERE status = 'open';

CREATE TABLE chat_messages (
    id            BIGSERIAL PRIMARY KEY,
    session_id    UUID NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    author        TEXT NOT NULL CHECK (author IN ('visitor','staff','system')),
    staff_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    body          TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 2000),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX chat_messages_session_idx ON chat_messages (session_id, id);
