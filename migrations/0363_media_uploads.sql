-- Uploaded images (staff and creators): owner and size, for per-user quotas.
CREATE TABLE IF NOT EXISTS media_files (
    id UUID PRIMARY KEY,
    owner_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    scope TEXT NOT NULL CHECK (scope IN ('staff', 'creator')),
    bytes INTEGER NOT NULL CHECK (bytes >= 0),
    width INTEGER NOT NULL DEFAULT 0,
    height INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS media_files_owner_idx ON media_files (owner_user_id, created_at DESC);

-- Creator profile banner (shown on /creators/{slug}).
ALTER TABLE creators ADD COLUMN IF NOT EXISTS banner_url TEXT NOT NULL DEFAULT '';
