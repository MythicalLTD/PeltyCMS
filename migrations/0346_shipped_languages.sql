-- All locales/*.json shipped with the server are built-in (cannot be deleted).
INSERT INTO languages (code, name, built_in, enabled) VALUES
    ('de', 'Deutsch', true, true),
    ('en', 'English', true, true),
    ('es', 'Español', true, true),
    ('fr', 'Français', true, true),
    ('nl', 'Nederlands', true, true),
    ('ro', 'Română', true, true)
ON CONFLICT (code) DO UPDATE SET
    built_in = true,
    name = EXCLUDED.name;
