-- Cron jobs an addon registers at runtime (power.cron-register, ABI 0.5).
-- Manifest jobs (capabilities.cron) are not stored here.
CREATE TABLE addon_cron_jobs (
    addon_id    TEXT NOT NULL,
    name        TEXT NOT NULL,
    schedule    TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (addon_id, name)
);
