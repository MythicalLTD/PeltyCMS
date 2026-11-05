-- Cron circuit breaker and catch-up policy per scheduled task.
-- failures: consecutive failed runs (reset on success); after
-- PELTY_CRON_MAX_FAILURES the task is disabled and tripped_at is set.
-- missed: what to do with a run missed while no scheduler was running.
ALTER TABLE automation_settings
    ADD COLUMN IF NOT EXISTS failures INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS tripped_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS tripped_error TEXT,
    ADD COLUMN IF NOT EXISTS missed TEXT NOT NULL DEFAULT 'skip' CHECK (missed IN ('skip', 'run_once'));
