DROP TRIGGER IF EXISTS vault_audit_log_no_update ON vault_audit_log;
DROP TRIGGER IF EXISTS vault_audit_log_no_delete ON vault_audit_log;
DROP FUNCTION IF EXISTS sprintly_block_audit_mutation();
DROP TABLE IF EXISTS vault_audit_log;
DROP TABLE IF EXISTS vault_access;
DROP TABLE IF EXISTS vault_items;
