DROP TABLE IF EXISTS backups;
DROP TRIGGER IF EXISTS admin_audit_log_no_update ON admin_audit_log;
DROP TRIGGER IF EXISTS admin_audit_log_no_delete ON admin_audit_log;
DROP TABLE IF EXISTS admin_audit_log;
DROP TRIGGER IF EXISTS webhooks_touch_updated_at ON webhooks;
DROP TABLE IF EXISTS webhooks;
