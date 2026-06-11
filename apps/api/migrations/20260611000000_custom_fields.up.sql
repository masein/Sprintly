-- F7 (second half): per-project custom field definitions + per-task values.
--
-- `custom_fields` is the schema (a project decides it wants a "Severity"
-- select or a "Story budget" number); `task_field_values` holds one value per
-- (task, field). Values are stored as canonical text — the field's `type`
-- says how to parse/validate them at the API layer:
--   text    free-form, trimmed, ≤500 chars
--   number  f64, canonical formatting ("3.5", not "3.50")
--   select  one of `options`, stored with the option's exact spelling
--   date    YYYY-MM-DD

CREATE TABLE custom_fields (
    id          uuid PRIMARY KEY,
    project_id  uuid NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        text NOT NULL,
    type        text NOT NULL CHECK (type IN ('text', 'number', 'select', 'date')),
    -- Select choices; empty for the other types.
    options     text[] NOT NULL DEFAULT '{}',
    created_at  timestamptz NOT NULL DEFAULT now()
);
-- Case-insensitive uniqueness of a field name within a project.
CREATE UNIQUE INDEX custom_fields_name_uniq
    ON custom_fields (project_id, lower(name));

CREATE TABLE task_field_values (
    task_id     uuid NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    field_id    uuid NOT NULL REFERENCES custom_fields(id) ON DELETE CASCADE,
    value       text NOT NULL,
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (task_id, field_id)
);
-- Board filtering: "all tasks where field X = value Y".
CREATE INDEX task_field_values_field_value_idx
    ON task_field_values (field_id, value);

-- ── search integration ──────────────────────────────────────────────────────
-- Fold field values into the task tsvector so cmd-K finds "Severity: critical"
-- tasks. Replaces the original function from 20260527030000_tasks.up.sql; the
-- existing BEFORE trigger on tasks picks the new body up automatically.
CREATE OR REPLACE FUNCTION sprintly_tasks_update_search() RETURNS trigger AS $$
BEGIN
    NEW.search_tsv :=
        setweight(to_tsvector('simple', coalesce(NEW.key, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.description, '')), 'B') ||
        setweight(to_tsvector('simple', coalesce(array_to_string(NEW.labels, ' '), '')), 'C') ||
        setweight(to_tsvector('simple', coalesce((
            SELECT string_agg(v.value, ' ')
            FROM task_field_values v
            WHERE v.task_id = NEW.id
        ), '')), 'C');
    RETURN NEW;
END
$$ LANGUAGE plpgsql;

-- When a value changes, poke the owning task with a self-assignment of a
-- column the tasks search trigger watches (`UPDATE OF ... labels`), so the
-- tsvector recomputes. AFTER row trigger: the new/changed value row is
-- already visible to the SELECT above.
CREATE FUNCTION sprintly_field_values_reindex_task() RETURNS trigger AS $$
BEGIN
    UPDATE tasks SET labels = labels
    WHERE id = COALESCE(NEW.task_id, OLD.task_id);
    RETURN NULL;
END
$$ LANGUAGE plpgsql;

CREATE TRIGGER task_field_values_reindex_trigger
    AFTER INSERT OR UPDATE OR DELETE ON task_field_values
    FOR EACH ROW EXECUTE FUNCTION sprintly_field_values_reindex_task();
