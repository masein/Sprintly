-- Files that belong to a project rather than to one task — roadmaps,
-- requirement briefs, contracts (QA report 6: "enable leaders to … upload
-- essential project documents directly within the dashboard"). Same two-phase
-- presigned upload as task attachments; its own table so a document's
-- lifecycle, and the leads-only rule for changing them, don't have to be
-- smuggled through a fake task.
CREATE TABLE project_documents (
    id           uuid        PRIMARY KEY,
    project_id   uuid        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    uploader_id  uuid        REFERENCES users (id) ON DELETE SET NULL,
    filename     text        NOT NULL,
    mime_type    text        NOT NULL,
    size_bytes   bigint,
    storage_key  text        NOT NULL UNIQUE,
    status       text        NOT NULL DEFAULT 'pending'
                             CHECK (status IN ('pending', 'ready', 'failed')),
    created_at   timestamptz NOT NULL DEFAULT now(),
    deleted_at   timestamptz
);

CREATE INDEX project_documents_project_idx
    ON project_documents (project_id) WHERE deleted_at IS NULL;
