-- WHMCS "Credit Card Expiry Notices": one notice per saved card.
ALTER TABLE payment_methods ADD COLUMN expiry_notified_at TIMESTAMPTZ;

-- WHMCS "Banned Emails": e-mail domains that may not register or order.
CREATE TABLE banned_email_domains (
    domain      TEXT PRIMARY KEY CHECK (domain ~ '^[a-z0-9.-]{1,253}$'),
    reason      TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 200),
    hits        BIGINT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
