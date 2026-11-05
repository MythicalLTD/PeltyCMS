-- SSL certificate monitoring for client domains (WHMCS "SSL Sync" / SSL status in client area).
CREATE TABLE ssl_checks (
    domain_id  UUID PRIMARY KEY REFERENCES domains(id) ON DELETE CASCADE,
    status     TEXT NOT NULL CHECK (status IN ('valid', 'expiring', 'expired', 'invalid', 'unreachable')),
    issuer     TEXT NOT NULL DEFAULT '',
    valid_from TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    error      TEXT NOT NULL DEFAULT '',
    checked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ssl_checks_checked_idx ON ssl_checks (checked_at);
