-- Cron expressions for scheduled tasks (company time zone). NULL keeps the
-- built-in interval; an existing start hour becomes "0 <hour> * * *".
ALTER TABLE automation_settings ADD COLUMN IF NOT EXISTS schedule TEXT;
UPDATE automation_settings SET schedule = '0 ' || run_hour || ' * * *', updated_at = now() WHERE run_hour IS NOT NULL AND schedule IS NULL;
