ALTER TABLE health_monitor.cost_daily ALTER COLUMN cost_daily_billed TYPE numeric(38,18), ALTER COLUMN cost_daily_effective TYPE numeric(38,18);
ALTER TABLE health_monitor.cost_import ADD COLUMN cost_import_billed_bytes bigint;
ALTER TABLE health_monitor.cost_import ADD CONSTRAINT cost_import_billed_bytes_valid CHECK (cost_import_billed_bytes IS NULL OR cost_import_billed_bytes BETWEEN 0 AND cost_import_reserved_bytes);

ALTER TABLE health_monitor.cost_daily DROP CONSTRAINT cost_daily_invoice_valid;
ALTER TABLE health_monitor.cost_daily ADD CONSTRAINT cost_daily_invoice_valid CHECK (cost_daily_invoice_month = '' OR cost_daily_invoice_month ~ '^[0-9]{4}(0[1-9]|1[0-2])$');
ALTER TABLE health_monitor.cost_import ADD COLUMN cost_import_attempted_at timestamptz;
CREATE INDEX cost_import_attempted ON health_monitor.cost_import (cost_import_source_id, cost_import_attempted_at);
ALTER TABLE health_monitor.cost_daily ADD COLUMN cost_daily_target varchar(128) NOT NULL DEFAULT '';
ALTER TABLE health_monitor.cost_daily ADD CONSTRAINT cost_daily_target_valid CHECK (cost_daily_target = '' OR cost_daily_target ~ '^[a-zA-Z0-9_-]{1,128}$');
CREATE INDEX cost_daily_target_day ON health_monitor.cost_daily (cost_daily_target, cost_daily_day);
