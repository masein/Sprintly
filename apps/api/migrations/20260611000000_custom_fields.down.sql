DROP TRIGGER IF EXISTS task_field_values_reindex_trigger ON task_field_values;
DROP FUNCTION IF EXISTS sprintly_field_values_reindex_task();

-- Restore the original tsvector function from 20260527030000_tasks.up.sql
-- (no field-value aggregation).
CREATE OR REPLACE FUNCTION sprintly_tasks_update_search() RETURNS trigger AS $$
BEGIN
    NEW.search_tsv :=
        setweight(to_tsvector('simple', coalesce(NEW.key, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.description, '')), 'B') ||
        setweight(to_tsvector('simple', coalesce(array_to_string(NEW.labels, ' '), '')), 'C');
    RETURN NEW;
END
$$ LANGUAGE plpgsql;

DROP TABLE IF EXISTS task_field_values;
DROP TABLE IF EXISTS custom_fields;
