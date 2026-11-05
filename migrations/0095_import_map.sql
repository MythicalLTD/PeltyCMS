-- Idempotent importer bookkeeping: one row per imported source record.
CREATE TABLE import_map (
    source      TEXT NOT NULL CHECK (source ~ '^[a-z][a-z0-9_]{1,40}$'),
    entity      TEXT NOT NULL CHECK (entity ~ '^[a-z][a-z0-9_]{1,40}$'),
    source_id   TEXT NOT NULL,
    target_id   UUID NOT NULL,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source, entity, source_id)
);
CREATE INDEX import_map_target_idx ON import_map (target_id);
