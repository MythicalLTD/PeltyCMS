-- Staff noticeboard (WHMCS "Staff Noticeboard" addon): short notices shown to all staff.
CREATE TABLE staff_notices (
    id         UUID PRIMARY KEY,
    author_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    body       TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 5000),
    pinned     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX staff_notices_order ON staff_notices (pinned DESC, created_at DESC);
