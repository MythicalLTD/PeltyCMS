-- Fraud review queue: staff decisions on held orders plus the request
-- signals (geo country, proxy headers, e-mail domain) a check was based on.
ALTER TABLE fraud_checks
  ADD COLUMN signals jsonb NOT NULL DEFAULT '{}'::jsonb,
  ADD COLUMN reviewed_at timestamptz,
  ADD COLUMN reviewed_by uuid REFERENCES users(id) ON DELETE SET NULL,
  ADD COLUMN decision text CHECK (decision IN ('approved', 'rejected')),
  ADD COLUMN review_note text NOT NULL DEFAULT '' CHECK (char_length(review_note) <= 2000),
  ADD CONSTRAINT fraud_checks_review_complete CHECK ((decision IS NULL) = (reviewed_at IS NULL));

CREATE INDEX fraud_checks_open_review ON fraud_checks (created_at DESC) WHERE verdict = 'review' AND decision IS NULL;
CREATE INDEX IF NOT EXISTS fraud_checks_order ON fraud_checks (order_id);
