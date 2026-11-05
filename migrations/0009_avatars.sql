-- Profile pictures, re-encoded server-side to PNG (256px and 64px squares).
CREATE TABLE user_avatars (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    large BYTEA NOT NULL,
    small BYTEA NOT NULL,
    version BIGINT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE users ADD COLUMN avatar_version BIGINT;
