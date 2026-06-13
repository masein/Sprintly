-- F10: OIDC / SSO. Store the federated identity so we can link an external
-- login to a local user. A user federated from an IdP has both columns set;
-- a purely-local user has both NULL. The pair is unique among live users so
-- one (issuer, subject) maps to at most one account.

ALTER TABLE users ADD COLUMN oidc_issuer  TEXT;
ALTER TABLE users ADD COLUMN oidc_subject TEXT;

CREATE UNIQUE INDEX users_oidc_identity_idx
    ON users (oidc_issuer, oidc_subject)
    WHERE oidc_issuer IS NOT NULL AND deleted_at IS NULL;
