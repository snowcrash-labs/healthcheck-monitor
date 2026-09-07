CREATE TYPE health_monitor.cost_provider AS ENUM ('gcp', 'aws', 'azure', 'external');
CREATE TABLE health_monitor.cost_source (
 cost_source_id uuid PRIMARY KEY DEFAULT uuidv7(),
 cost_source_name varchar(128) NOT NULL UNIQUE,
 cost_source_provider health_monitor.cost_provider NOT NULL,
 cost_source_scope varchar(128) NOT NULL,
 cost_source_imported_at timestamptz,
 cost_source_revision uuid,
 cost_source_fault varchar(128),
 CONSTRAINT cost_source_unique_scope UNIQUE (cost_source_provider, cost_source_scope),
 CONSTRAINT cost_source_name_valid CHECK (cost_source_name ~ '^[a-zA-Z0-9_-]{1,128}$')
);
CREATE TABLE health_monitor.cost_import (
 cost_import_id uuid PRIMARY KEY DEFAULT uuidv7(),
 cost_import_source_id uuid NOT NULL REFERENCES health_monitor.cost_source ON DELETE CASCADE,
 cost_import_started_at timestamptz NOT NULL DEFAULT now(),
 cost_import_published_at timestamptz,
 cost_import_from date NOT NULL,
 cost_import_to date NOT NULL,
 cost_import_reserved_bytes bigint NOT NULL,
 CONSTRAINT cost_import_period CHECK (cost_import_to > cost_import_from AND cost_import_to - cost_import_from <= 400),
 CONSTRAINT cost_import_bytes CHECK (cost_import_reserved_bytes > 0)
);
CREATE INDEX cost_import_source ON health_monitor.cost_import (cost_import_source_id, cost_import_started_at);
CREATE INDEX cost_import_started ON health_monitor.cost_import (cost_import_started_at);
ALTER TABLE health_monitor.cost_source ADD CONSTRAINT cost_source_revision_fk FOREIGN KEY (cost_source_revision) REFERENCES health_monitor.cost_import;
CREATE INDEX cost_source_revision ON health_monitor.cost_source (cost_source_revision);
CREATE TABLE health_monitor.cost_partition (
 cost_partition_id uuid PRIMARY KEY DEFAULT uuidv7(),
 cost_partition_source_id uuid NOT NULL REFERENCES health_monitor.cost_source ON DELETE CASCADE,
 cost_partition_day date NOT NULL,
 cost_partition_import_id uuid NOT NULL REFERENCES health_monitor.cost_import ON DELETE CASCADE,
 CONSTRAINT cost_partition_unique UNIQUE (cost_partition_source_id, cost_partition_day)
);
CREATE INDEX cost_partition_import ON health_monitor.cost_partition (cost_partition_import_id, cost_partition_day);
CREATE INDEX cost_partition_day ON health_monitor.cost_partition (cost_partition_day);
CREATE TABLE health_monitor.cost_daily (
 cost_daily_id uuid PRIMARY KEY DEFAULT uuidv7(),
 cost_daily_import_id uuid NOT NULL REFERENCES health_monitor.cost_import ON DELETE CASCADE,
 cost_daily_key varchar(64) NOT NULL,
 cost_daily_day date NOT NULL,
 cost_daily_invoice_month varchar(6) NOT NULL,
 cost_daily_scope varchar(128) NOT NULL,
 cost_daily_region varchar(128) NOT NULL,
 cost_daily_product varchar(256) NOT NULL,
 cost_daily_resource varchar(2048) NOT NULL,
 cost_daily_category varchar(128) NOT NULL,
 cost_daily_currency varchar(3) NOT NULL,
 cost_daily_billed numeric(28,9) NOT NULL,
 cost_daily_effective numeric(28,9),
 CONSTRAINT cost_daily_identity UNIQUE (cost_daily_import_id, cost_daily_key),
 CONSTRAINT cost_daily_key_valid CHECK (cost_daily_key ~ '^[a-f0-9]{64}$'),
 CONSTRAINT cost_daily_invoice_valid CHECK (cost_daily_invoice_month ~ '^[0-9]{4}(0[1-9]|1[0-2])$'),
 CONSTRAINT cost_daily_currency_valid CHECK (cost_daily_currency IN ('USD','EUR','GBP','JPY','CAD','AUD','NZD','CHF','INR','KRW','SGD','BRL','MXN','CNY','HKD','TWD','SEK','NOK','DKK','PLN','ZAR','IDR','MYR','THB','TRY','ILS','AED','SAR','CLP','COP','PEN')),
 CONSTRAINT cost_daily_dimensions CHECK (octet_length(cost_daily_scope) <= 128 AND octet_length(cost_daily_region) <= 128 AND octet_length(cost_daily_product) BETWEEN 1 AND 256 AND octet_length(cost_daily_resource) <= 2048 AND octet_length(cost_daily_category) BETWEEN 1 AND 128)
);
CREATE INDEX cost_daily_day_currency ON health_monitor.cost_daily (cost_daily_day, cost_daily_currency, cost_daily_import_id);
CREATE INDEX cost_daily_scope_day ON health_monitor.cost_daily (cost_daily_scope, cost_daily_day);
CREATE INDEX cost_daily_product_day ON health_monitor.cost_daily (cost_daily_product, cost_daily_day);
CREATE INDEX cost_daily_region_day ON health_monitor.cost_daily (cost_daily_region, cost_daily_day);
CREATE INDEX cost_daily_resource_day ON health_monitor.cost_daily (cost_daily_resource, cost_daily_day);
CREATE INDEX cost_daily_category_day ON health_monitor.cost_daily (cost_daily_category, cost_daily_day);
CREATE INDEX cost_daily_invoice ON health_monitor.cost_daily (cost_daily_invoice_month);

