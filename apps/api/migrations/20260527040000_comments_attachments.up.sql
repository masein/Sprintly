-- ─────────────────────────────────────────────────────────────────────────────
-- M3 phase B — comments, reactions, attachments.
--
-- Threading depth is exactly one. A comment with parent_comment_id set
-- cannot itself be a reply target. We enforce that with a trigger so the
-- model stays simple — no UI affordance to make it deeper than two-level
-- threads, no schema invariant that allows it.
--
-- Reactions can target either a task OR a comment, never both. Enforced
-- with a CHECK; uniqueness is per (target, user, emoji).
--
-- Attachments are two-phase:
--    1. Server inserts a row with status='pending', returns a presigned PUT.
--    2. Client uploads to MinIO, then POSTs /attachments/:id/complete →
--       status='ready' + checksum.
-- Orphaned 'pending' rows are pruned by a daily job (jobs table — added
-- as part of M3-A schema, runner lands later).
-- ─────────────────────────────────────────────────────────────────────────────

-- ── task_comments ──────────────────────────────────────────────────────────
CREATE TABLE task_comments (
    id                UUID PRIMARY KEY,
    task_id           UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    author_id         UUID REFERENCES users(id) ON DELETE SET NULL,
    parent_comment_id UUID REFERENCES task_comments(id) ON DELETE CASCADE,
    body              TEXT NOT NULL,
    edited_at         TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ
);
CREATE INDEX task_comments_task_idx
    ON task_comments (task_id, created_at) WHERE deleted_at IS NULL;
CREATE INDEX task_comments_parent_idx
    ON task_comments (parent_comment_id) WHERE deleted_at IS NULL;

-- One-level threading guard. If parent_comment_id refers to a row that
-- itself has parent_comment_id, refuse.
CREATE OR REPLACE FUNCTION sprintly_enforce_one_level_threads()
RETURNS trigger AS $$
DECLARE
    parents_parent UUID;
BEGIN
    IF NEW.parent_comment_id IS NULL THEN
        RETURN NEW;
    END IF;
    SELECT parent_comment_id INTO parents_parent
    FROM   task_comments
    WHERE  id = NEW.parent_comment_id;
    IF parents_parent IS NOT NULL THEN
        RAISE EXCEPTION 'replies can be one level deep only';
    END IF;
    RETURN NEW;
END
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_comments_threading_guard
    BEFORE INSERT OR UPDATE OF parent_comment_id ON task_comments
    FOR EACH ROW EXECUTE FUNCTION sprintly_enforce_one_level_threads();

CREATE TRIGGER task_comments_touch_updated_at
    BEFORE UPDATE ON task_comments
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();

-- ── task_reactions ─────────────────────────────────────────────────────────
-- Exactly one of (task_id, comment_id) is non-null.
CREATE TABLE task_reactions (
    id            UUID PRIMARY KEY,
    task_id       UUID REFERENCES tasks(id) ON DELETE CASCADE,
    comment_id    UUID REFERENCES task_comments(id) ON DELETE CASCADE,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    emoji         TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    CHECK ((task_id IS NULL) <> (comment_id IS NULL))
);
CREATE UNIQUE INDEX task_reactions_task_unique
    ON task_reactions (task_id, user_id, emoji) WHERE task_id IS NOT NULL;
CREATE UNIQUE INDEX task_reactions_comment_unique
    ON task_reactions (comment_id, user_id, emoji) WHERE comment_id IS NOT NULL;

-- ── task_attachments ───────────────────────────────────────────────────────
CREATE TABLE task_attachments (
    id            UUID PRIMARY KEY,
    task_id       UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    uploader_id   UUID REFERENCES users(id) ON DELETE SET NULL,

    filename      TEXT NOT NULL,
    mime_type     TEXT NOT NULL,
    size_bytes    BIGINT,
    storage_key   TEXT NOT NULL UNIQUE,    -- s3 object key under the bucket
    checksum      TEXT,                     -- sha256, hex, set on complete
    status        TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending', 'ready', 'failed')),

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ
);
CREATE INDEX task_attachments_task_idx
    ON task_attachments (task_id) WHERE deleted_at IS NULL;
CREATE INDEX task_attachments_pending_idx
    ON task_attachments (created_at)
    WHERE status = 'pending' AND deleted_at IS NULL;

CREATE TRIGGER task_attachments_touch_updated_at
    BEFORE UPDATE ON task_attachments
    FOR EACH ROW EXECUTE FUNCTION sprintly_touch_updated_at();
