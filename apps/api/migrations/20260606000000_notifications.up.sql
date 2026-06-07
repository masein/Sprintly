-- In-app notifications: @mentions, task assignment, watched-task comments.
CREATE TABLE notifications (
    id          uuid PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_id    uuid REFERENCES users(id) ON DELETE SET NULL,
    kind        text NOT NULL CHECK (kind IN ('mention', 'assigned', 'comment')),
    title       text NOT NULL,
    body        text,
    link        text,
    task_id     uuid REFERENCES tasks(id) ON DELETE CASCADE,
    read_at     timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- Inbox list (newest first) and the unread badge query.
CREATE INDEX notifications_user_idx ON notifications (user_id, created_at DESC);
CREATE INDEX notifications_user_unread_idx
    ON notifications (user_id, created_at DESC)
    WHERE read_at IS NULL;
