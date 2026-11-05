-- WHMCS API gaps: UpdateTransaction (staff note on a payment).
ALTER TABLE payments ADD COLUMN IF NOT EXISTS admin_note TEXT CHECK (admin_note IS NULL OR length(admin_note) <= 2000);
