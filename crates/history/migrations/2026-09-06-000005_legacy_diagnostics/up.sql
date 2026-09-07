-- Native transition history has no historical cloud scope or triggering payload.
-- Keep those fields unknown rather than assigning today's configuration to old events.
WITH ordered AS (
    SELECT *, lead(finding_event_at) OVER (
        PARTITION BY finding_event_finding ORDER BY finding_event_at, finding_event_id
    ) AS next_event_at
    FROM health_monitor.finding_event
    WHERE finding_event_at >= clock_timestamp() - interval '7 days'
), legacy AS (
    SELECT *, CASE WHEN finding_event_kind IN ('recovered', 'removed') THEN finding_event_at ELSE next_event_at END AS closed_at,
        CASE WHEN finding_event_kind = 'recovered' THEN 'recovered' WHEN finding_event_kind = 'removed' THEN 'removed' ELSE 'active' END AS state
    FROM ordered
), projected AS (
    SELECT *, jsonb_build_object(
        'id', finding_event_id::text, 'identity', finding_event_finding,
        'scope', jsonb_build_object('target', finding_event_target, 'provider', 'unknown', 'scope', 'unknown'),
        'location', '{}'::jsonb, 'check', NULL, 'resource', finding_event_resource,
        'observed_at', finding_event_observed_at, 'last_observed_at', finding_event_observed_at,
        'valid_until', NULL, 'closed_at', closed_at, 'stale', true,
        'details', jsonb_build_object(
            'kind', 'finding', 'rule', finding_event_rule, 'severity', finding_event_severity::text,
            'state', state, 'first_detected_at', NULL, 'expected', finding_event_expected::text,
            'confidence', finding_event_confidence::text, 'facts', '[]'::jsonb, 'links', '[]'::jsonb, 'legacy', true
        )
    ) AS payload FROM legacy
)
INSERT INTO health_monitor.query_record (
    query_record_key, query_record_identity, query_record_category, query_record_provider,
    query_record_target, query_record_scope, query_record_resource, query_record_severity,
    query_record_state, query_record_from, query_record_to, query_record_closed_at, query_record_payload
)
SELECT finding_event_key, finding_event_finding, 'finding', 'unknown', finding_event_target,
    'unknown', finding_event_resource, finding_event_severity, state, finding_event_observed_at,
    finding_event_observed_at, closed_at, payload
FROM projected WHERE octet_length(payload::text) <= 16384
ON CONFLICT (query_record_key) DO NOTHING;
