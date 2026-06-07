-- F2: outbound webhook delivery.
--
-- Signing an outbound payload needs the *raw* secret, but the scaffold only
-- kept a one-way hash. Store the secret encrypted at rest (XChaCha20-Poly1305
-- under the per-project vault key, AAD = webhook id) so it can be recovered to
-- compute the HMAC, and stop requiring the legacy hash.
ALTER TABLE webhooks
    ADD COLUMN secret_ciphertext bytea,
    ADD COLUMN secret_nonce      bytea,
    ALTER COLUMN secret_hash DROP NOT NULL;

-- Per-attempt delivery log for observability in the admin UI.
CREATE TABLE webhook_deliveries (
    id            uuid PRIMARY KEY,
    webhook_id    uuid NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event         text NOT NULL,
    status_code   integer,
    ok            boolean NOT NULL DEFAULT false,
    error         text,
    attempt       integer NOT NULL DEFAULT 1,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX webhook_deliveries_webhook_idx
    ON webhook_deliveries (webhook_id, created_at DESC);
