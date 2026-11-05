-- Telegram, SMS (HTTP gateway or email-to-SMS) notification channels, HMAC
-- signed generic webhooks with a per-attempt delivery log.
ALTER TABLE notification_rules DROP CONSTRAINT IF EXISTS notification_rules_provider_check;
ALTER TABLE notification_rules DROP CONSTRAINT IF EXISTS notification_rules_url_check;
ALTER TABLE notification_rules ADD CONSTRAINT notification_rules_provider_check
    CHECK (provider IN ('slack','discord','teams','webhook','telegram','sms_webhook','sms_email'));
ALTER TABLE notification_rules ADD CONSTRAINT notification_rules_url_check
    CHECK ((provider = 'sms_email' AND url = '') OR (url ~ '^https://' AND length(url) <= 500));
ALTER TABLE notification_rules ADD COLUMN target TEXT NOT NULL DEFAULT '' CHECK (length(target) <= 254);
ALTER TABLE notification_rules ADD COLUMN secret_sealed BYTEA;

CREATE TABLE notification_deliveries (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rule_id         UUID NOT NULL REFERENCES notification_rules(id) ON DELETE CASCADE,
    event           TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','delivered','failed')),
    attempts        INT NOT NULL DEFAULT 0,
    response_status INT,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at    TIMESTAMPTZ
);
CREATE INDEX notification_deliveries_rule_idx ON notification_deliveries (rule_id, created_at DESC);
