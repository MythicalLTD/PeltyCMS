-- Server health (last connection test) and a log of every module call.
ALTER TABLE servers ADD COLUMN last_check_ok BOOLEAN;
ALTER TABLE servers ADD COLUMN last_check_error TEXT NOT NULL DEFAULT '' CHECK (length(last_check_error) <= 200);
ALTER TABLE servers ADD COLUMN last_checked_at TIMESTAMPTZ;

CREATE TABLE module_log (
    id          BIGSERIAL PRIMARY KEY,
    service_id  UUID REFERENCES services(id) ON DELETE CASCADE,
    server_id   UUID REFERENCES servers(id) ON DELETE SET NULL,
    module      TEXT NOT NULL CHECK (length(module) <= 64),
    action      TEXT NOT NULL CHECK (length(action) <= 64),
    actor_id    UUID,
    source      TEXT NOT NULL DEFAULT 'queue' CHECK (source IN ('queue','admin','client','system')),
    ok          BOOLEAN NOT NULL,
    error       TEXT NOT NULL DEFAULT '' CHECK (length(error) <= 2000),
    -- Output with secrets (passwords, tickets) redacted.
    output      JSONB NOT NULL DEFAULT '{}'::jsonb,
    duration_ms INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX module_log_service_idx ON module_log (service_id, created_at DESC);
CREATE INDEX module_log_created_idx ON module_log (created_at DESC);
CREATE INDEX services_remote_id_idx ON services (remote_id) WHERE remote_id IS NOT NULL;
