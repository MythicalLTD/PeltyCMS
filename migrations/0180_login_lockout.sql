-- Per-account login backoff: after LOCK_AFTER consecutive failures the
-- account is locked for an exponentially growing window (capped).
ALTER TABLE users
    ADD COLUMN failed_logins INT NOT NULL DEFAULT 0,
    ADD COLUMN locked_until  TIMESTAMPTZ;
