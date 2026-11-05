-- ICANN Registrar Accreditation Agreement (WHOIS accuracy program): the
-- registrant's e-mail must be verified within 15 days of a registration,
-- transfer or registrant e-mail change. One row per domain (latest request).
CREATE TABLE registrant_verifications (
    domain_id   UUID PRIMARY KEY REFERENCES domains(id) ON DELETE CASCADE,
    email       TEXT NOT NULL,
    token_hash  BYTEA NOT NULL UNIQUE,
    status      TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'verified', 'overdue')),
    deadline    DATE NOT NULL,
    sent_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    reminders   INT NOT NULL DEFAULT 0,
    verified_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX registrant_verifications_email_idx ON registrant_verifications (lower(email)) WHERE status = 'verified';
CREATE INDEX registrant_verifications_pending_idx ON registrant_verifications (deadline) WHERE status <> 'verified';
