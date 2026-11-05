-- Debit notes: an extra charge issued against an existing invoice (the
-- counterpart of a credit note). Linked via credit_for (= "related invoice").
DO $$
DECLARE c record;
BEGIN
  FOR c IN SELECT conname FROM pg_constraint
           WHERE conrelid = 'invoices'::regclass AND contype = 'c'
             AND pg_get_constraintdef(oid) LIKE '%credit_note%' AND pg_get_constraintdef(oid) LIKE '%kind%'
  LOOP
    EXECUTE format('ALTER TABLE invoices DROP CONSTRAINT %I', c.conname);
  END LOOP;
  FOR c IN SELECT conname FROM pg_constraint
           WHERE conrelid = 'legal_entity_sequences'::regclass AND contype = 'c'
             AND pg_get_constraintdef(oid) LIKE '%credit_note%'
  LOOP
    EXECUTE format('ALTER TABLE legal_entity_sequences DROP CONSTRAINT %I', c.conname);
  END LOOP;
END $$;
ALTER TABLE invoices ADD CONSTRAINT invoices_kind_check CHECK (kind IN ('invoice', 'credit_note', 'debit_note'));
ALTER TABLE invoices ADD COLUMN note_reason TEXT NOT NULL DEFAULT '' CHECK (length(note_reason) <= 2000);
ALTER TABLE legal_entity_sequences ADD CONSTRAINT legal_entity_sequences_kind_check CHECK (kind IN ('invoice', 'credit_note', 'debit_note'));
INSERT INTO invoice_sequences VALUES ('debit_note', 1) ON CONFLICT DO NOTHING;
CREATE INDEX IF NOT EXISTS invoices_credit_for_idx ON invoices (credit_for) WHERE credit_for IS NOT NULL;
