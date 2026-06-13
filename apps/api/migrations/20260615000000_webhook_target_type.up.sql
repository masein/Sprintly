-- F2 (deferred half): first-class Slack/Discord delivery targets (ADR 0002).
-- A target type picks the body format + auth at delivery time. 'outbound' is
-- the existing generic signed delivery; chat targets format a message and POST
-- to the URL (which is itself the secret — no HMAC).
ALTER TABLE webhooks
    ADD COLUMN target_type text NOT NULL DEFAULT 'outbound'
        CHECK (target_type IN ('outbound', 'slack', 'discord'));
