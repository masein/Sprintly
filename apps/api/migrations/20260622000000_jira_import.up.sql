-- Native Jira import (extends F16).
--
-- A source-system reference on tasks so a re-import updates the same card
-- instead of minting a duplicate. Nullable — only set for tasks created by an
-- external importer (e.g. the Jira issue key "PROJ-123"). Native tasks leave
-- it NULL.

ALTER TABLE tasks ADD COLUMN external_ref TEXT;

COMMENT ON COLUMN tasks.external_ref IS
    'Source-system reference (e.g. Jira issue key) for idempotent re-import; NULL for native tasks.';

-- At most one task per (project, external key), ignoring soft-deleted rows —
-- this is what lets re-import dedupe by Jira key.
CREATE UNIQUE INDEX tasks_external_ref_uniq
    ON tasks (project_id, external_ref)
    WHERE external_ref IS NOT NULL AND deleted_at IS NULL;
