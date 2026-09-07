DELETE FROM health_monitor.query_record WHERE query_record_provider = 'unknown' AND query_record_payload->'details'->>'legacy' = 'true';
