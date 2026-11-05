-- KYC: identity documents uploaded by customers, reviewed by staff.
ALTER TABLE customers
    ADD COLUMN kyc_status TEXT NOT NULL DEFAULT 'none' CHECK (kyc_status IN ('none','pending','verified','rejected')),
    ADD COLUMN kyc_note TEXT NOT NULL DEFAULT '' CHECK (length(kyc_note) <= 1000),
    ADD COLUMN kyc_reviewed_at TIMESTAMPTZ;

CREATE TABLE kyc_documents (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    customer_id   UUID NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('identity','address','company','other')),
    filename      TEXT NOT NULL CHECK (length(filename) BETWEEN 1 AND 120),
    content_type  TEXT NOT NULL,
    size_bytes    INT NOT NULL CHECK (size_bytes > 0),
    data          BYTEA NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
    review_note   TEXT NOT NULL DEFAULT '' CHECK (length(review_note) <= 1000),
    reviewed_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at   TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX kyc_documents_customer ON kyc_documents (customer_id, created_at DESC);
CREATE INDEX kyc_documents_pending ON kyc_documents (created_at) WHERE status = 'pending';
