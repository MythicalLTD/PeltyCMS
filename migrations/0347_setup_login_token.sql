-- One-click post-setup admin sign-in uses purpose setup_login.
ALTER TABLE user_tokens DROP CONSTRAINT IF EXISTS user_tokens_purpose_check;
ALTER TABLE user_tokens ADD CONSTRAINT user_tokens_purpose_check
    CHECK (purpose IN ('password_reset', 'email_verify', 'setup_login'));
