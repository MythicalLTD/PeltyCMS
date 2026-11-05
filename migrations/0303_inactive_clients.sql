-- Last login, kept even after sessions are purged; used by the
-- inactive-client retention job.
ALTER TABLE users ADD COLUMN last_login_at TIMESTAMPTZ;
UPDATE users u SET last_login_at = (SELECT max(created_at) FROM sessions s WHERE s.user_id = u.id);
CREATE FUNCTION sessions_touch_last_login() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE users SET last_login_at = NEW.created_at WHERE id = NEW.user_id;
    RETURN NEW;
END $$;
CREATE TRIGGER sessions_last_login AFTER INSERT ON sessions FOR EACH ROW EXECUTE FUNCTION sessions_touch_last_login();
