-- User avatars: a chosen generated-avatar style + an optional override seed.
-- The existing nullable `avatar_url` already holds an uploaded/linked image;
-- these two columns describe the *generated* avatar a user picks instead.
--
--   avatar_url   set  → render that image.
--   avatar_url   null → render a generated avatar from
--                       (avatar_seed ?? user id) in `avatar_style` (?? default).
ALTER TABLE users
    ADD COLUMN avatar_style TEXT,
    ADD COLUMN avatar_seed  TEXT;

COMMENT ON COLUMN users.avatar_style IS
    'Chosen generated-avatar style (beaver/robot/identicon/glyph); NULL = default.';
COMMENT ON COLUMN users.avatar_seed IS
    'Override seed for the generated avatar; NULL = derive deterministically from the user id.';

-- Keep the style values honest; a new style is an additive migration.
ALTER TABLE users
    ADD CONSTRAINT users_avatar_style_chk
    CHECK (avatar_style IS NULL OR avatar_style IN ('beaver', 'robot', 'identicon', 'glyph'));
