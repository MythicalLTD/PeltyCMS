-- Mass mail scheduling: a campaign can wait in 'scheduled' until scheduled_at.
ALTER TABLE mail_campaigns DROP CONSTRAINT IF EXISTS mail_campaigns_status_check;
ALTER TABLE mail_campaigns ADD CONSTRAINT mail_campaigns_status_check
    CHECK (status IN ('draft','scheduled','sending','sent','cancelled'));
ALTER TABLE mail_campaigns ADD COLUMN scheduled_at TIMESTAMPTZ;
