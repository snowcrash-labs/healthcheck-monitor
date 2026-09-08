-- Widening scale can rewrite an existing billing table; bound that work separately from normal reads.
SET LOCAL statement_timeout = '60s';
SET LOCAL lock_timeout = '5s';
ALTER TABLE health_monitor.cost_daily
    ALTER COLUMN cost_daily_billed TYPE numeric(58,38),
    ALTER COLUMN cost_daily_effective TYPE numeric(58,38);
