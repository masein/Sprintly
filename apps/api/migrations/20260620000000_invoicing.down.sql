DROP TABLE IF EXISTS invoice_lines;
DROP TABLE IF EXISTS invoices;
DROP INDEX IF EXISTS projects_client_idx;
ALTER TABLE projects DROP COLUMN IF EXISTS client_id;
DROP TABLE IF EXISTS clients;
