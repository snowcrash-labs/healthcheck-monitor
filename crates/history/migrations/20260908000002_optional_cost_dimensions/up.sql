ALTER TABLE health_monitor.cost_daily ADD COLUMN IF NOT EXISTS cost_daily_target varchar(128);
ALTER TABLE health_monitor.cost_daily
 ALTER COLUMN cost_daily_target DROP NOT NULL, ALTER COLUMN cost_daily_target DROP DEFAULT,
 ALTER COLUMN cost_daily_invoice_month DROP NOT NULL,
 ALTER COLUMN cost_daily_scope DROP NOT NULL, ALTER COLUMN cost_daily_region DROP NOT NULL,
 ALTER COLUMN cost_daily_resource DROP NOT NULL, ALTER COLUMN cost_daily_category DROP NOT NULL;
UPDATE health_monitor.cost_daily SET
 cost_daily_target = NULLIF(cost_daily_target, ''),
 cost_daily_invoice_month = NULLIF(cost_daily_invoice_month, ''),
 cost_daily_scope = NULLIF(cost_daily_scope, ''),
 cost_daily_region = NULLIF(cost_daily_region, ''),
 cost_daily_resource = NULLIF(cost_daily_resource, ''),
 cost_daily_category = NULLIF(cost_daily_category, '');
ALTER TABLE health_monitor.cost_daily DROP CONSTRAINT IF EXISTS cost_daily_target_valid, DROP CONSTRAINT cost_daily_invoice_valid, DROP CONSTRAINT cost_daily_dimensions;
ALTER TABLE health_monitor.cost_daily ADD CONSTRAINT cost_daily_target_valid CHECK (cost_daily_target IS NULL OR cost_daily_target ~ '^[a-zA-Z0-9_-]{1,128}$');
ALTER TABLE health_monitor.cost_daily ADD CONSTRAINT cost_daily_invoice_valid CHECK (cost_daily_invoice_month IS NULL OR cost_daily_invoice_month ~ '^[0-9]{4}(0[1-9]|1[0-2])$');
ALTER TABLE health_monitor.cost_daily ADD CONSTRAINT cost_daily_dimensions CHECK (
 (cost_daily_scope IS NULL OR octet_length(cost_daily_scope) BETWEEN 1 AND 128) AND
 (cost_daily_region IS NULL OR octet_length(cost_daily_region) BETWEEN 1 AND 128) AND
 octet_length(cost_daily_product) BETWEEN 1 AND 256 AND
 (cost_daily_resource IS NULL OR octet_length(cost_daily_resource) BETWEEN 1 AND 2048) AND
 (cost_daily_category IS NULL OR octet_length(cost_daily_category) BETWEEN 1 AND 128)
);
CREATE INDEX IF NOT EXISTS cost_daily_target_day ON health_monitor.cost_daily (cost_daily_target, cost_daily_day);
