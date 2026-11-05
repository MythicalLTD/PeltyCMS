-- Match the admin user list ordering exactly (created_at DESC, id) so the
-- page is an index scan with no sort.
DROP INDEX IF EXISTS users_created_at_idx;
CREATE INDEX users_created_at_id_idx ON users (created_at DESC, id);
