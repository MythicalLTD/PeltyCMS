-- Optional full HTML body for email templates. Empty/NULL keeps the
-- plain-text → branded HTML path. Merge fields in HTML are still escaped.
ALTER TABLE email_templates
    ADD COLUMN html TEXT
    CHECK (html IS NULL OR (char_length(html) BETWEEN 1 AND 100000));
