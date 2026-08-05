-- Password entries need a username alongside the secret: the `name` column
-- carries the site/host/url, so the account it belongs to had nowhere to live.
-- Empty for every other kind (and for password entries that don't need one).
ALTER TABLE vault_items ADD COLUMN username TEXT NOT NULL DEFAULT '';
