CREATE VIEW health_monitor.query_gap AS
WITH checks AS (
    SELECT query_record_id, query_record_identity, query_record_target, query_record_provider,
        query_record_scope, query_record_check, query_record_from,
        coalesce((query_record_payload->'details'->>'complete')::boolean, false) AS complete,
        (query_record_payload->>'valid_until')::timestamptz AS valid_until,
        lead(query_record_from) OVER (PARTITION BY query_record_identity ORDER BY query_record_from, query_record_id) AS next_at
    FROM health_monitor.query_record
    WHERE query_record_category = 'check'
        AND coalesce((query_record_payload->'details'->>'required')::boolean, true)
), gaps AS (
    SELECT query_record_id AS query_gap_id, query_record_target AS query_gap_target,
        query_record_provider AS query_gap_provider, query_record_scope AS query_gap_scope,
        query_record_check AS query_gap_check,
        CASE WHEN complete THEN coalesce(valid_until, query_record_from) ELSE query_record_from END AS query_gap_from,
        coalesce(next_at, clock_timestamp()) AS query_gap_to
    FROM checks
)
SELECT * FROM gaps WHERE query_gap_from < query_gap_to;
