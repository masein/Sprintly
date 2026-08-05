-- Saved JQL queries ("filters"/templates). Owned by a user; optionally shared
-- with everyone, which is how a team gets a common set of views without each
-- person retyping the same query.
CREATE TABLE saved_queries (
    id         uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       text        NOT NULL,
    jql        text        NOT NULL,
    is_shared  boolean     NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT saved_queries_name_len CHECK (char_length(name) BETWEEN 1 AND 60),
    CONSTRAINT saved_queries_jql_len  CHECK (char_length(jql) BETWEEN 1 AND 2000)
);

-- One name per person, case-insensitively: re-saving under the same name is a
-- conflict the API turns into a friendly 409 rather than a silent duplicate.
CREATE UNIQUE INDEX saved_queries_user_name_uniq
    ON saved_queries (user_id, lower(name));

CREATE INDEX saved_queries_shared_idx
    ON saved_queries (is_shared) WHERE is_shared;
