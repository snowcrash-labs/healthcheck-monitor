ALTER TABLE health_monitor.cost_import DROP COLUMN cost_import_billed_bytes;
-- Preserve exact amounts on binary rollback; narrowing numeric precision could lose charges.
ALTER TABLE health_monitor.cost_import DROP COLUMN cost_import_attempted_at;
ALTER TABLE health_monitor.cost_daily DROP COLUMN cost_daily_target;
