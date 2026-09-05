DO $$ BEGIN
    IF current_setting('server_version_num')::integer < 180006 THEN
        RAISE EXCEPTION 'PostgreSQL 18.6 or newer is required';
    END IF;
END $$;

CREATE SCHEMA health_monitor;
CREATE TYPE health_monitor.event_kind AS ENUM ('new', 'worsened', 'recovered', 'stale', 'removed', 'reappeared');
CREATE TYPE health_monitor.severity AS ENUM ('info', 'warning', 'error');
CREATE TYPE health_monitor.expected AS ENUM ('active', 'dormant', 'scale_to_zero', 'suspended');
CREATE TYPE health_monitor.confidence AS ENUM ('direct', 'correlated', 'insufficient');
CREATE TYPE health_monitor.check_kind AS ENUM ('preflight', 'discovery', 'inventory', 'kubernetes', 'edge', 'managed', 'queues', 'releases', 'github', 'metrics', 'logs', 'alerts', 'slo', 'flows');

CREATE TABLE health_monitor.configuration (
    configuration_id uuid DEFAULT uuidv7() CONSTRAINT configuration_pkey PRIMARY KEY,
    configuration_revision varchar(64) NOT NULL CONSTRAINT configuration_revision_key UNIQUE,
    configuration_seen_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT configuration_id_v7 CHECK (uuid_extract_version(configuration_id) IS NOT DISTINCT FROM 7),
    CONSTRAINT configuration_revision_format CHECK (configuration_revision ~ '^[0-9a-f]{64}$')
);
CREATE INDEX configuration_seen_at_idx ON health_monitor.configuration (configuration_seen_at);

CREATE TABLE health_monitor.check_run (
    check_run_id uuid DEFAULT uuidv7() CONSTRAINT check_run_pkey PRIMARY KEY,
    check_run_configuration_id uuid NOT NULL CONSTRAINT check_run_configuration_fkey REFERENCES health_monitor.configuration(configuration_id) ON DELETE CASCADE,
    check_run_key varchar(64) NOT NULL CONSTRAINT check_run_key_key UNIQUE,
    check_run_target varchar(128) NOT NULL,
    check_run_check health_monitor.check_kind NOT NULL,
    check_run_started_at timestamptz NOT NULL,
    check_run_finished_at timestamptz NOT NULL,
    check_run_complete boolean NOT NULL,
    check_run_observations bigint NOT NULL,
    check_run_required_failures bigint NOT NULL,
    CONSTRAINT check_run_id_v7 CHECK (uuid_extract_version(check_run_id) IS NOT DISTINCT FROM 7),
    CONSTRAINT check_run_key_format CHECK (check_run_key ~ '^[0-9a-f]{64}$'),
    CONSTRAINT check_run_target_nonempty CHECK (char_length(check_run_target) > 0),
    CONSTRAINT check_run_time_order CHECK (check_run_finished_at >= check_run_started_at),
    CONSTRAINT check_run_observations_nonnegative CHECK (check_run_observations >= 0),
    CONSTRAINT check_run_failures_nonnegative CHECK (check_run_required_failures >= 0)
);
CREATE INDEX check_run_configuration_idx ON health_monitor.check_run (check_run_configuration_id);
CREATE INDEX check_run_finished_at_id_idx ON health_monitor.check_run (check_run_finished_at DESC, check_run_id DESC);
CREATE INDEX check_run_target_finished_at_idx ON health_monitor.check_run (check_run_target, check_run_finished_at DESC);
CREATE INDEX check_run_check_finished_at_idx ON health_monitor.check_run (check_run_check, check_run_finished_at DESC);

CREATE TABLE health_monitor.finding_event (
    finding_event_id uuid DEFAULT uuidv7() CONSTRAINT finding_event_pkey PRIMARY KEY,
    finding_event_configuration_id uuid NOT NULL CONSTRAINT finding_event_configuration_fkey REFERENCES health_monitor.configuration(configuration_id) ON DELETE CASCADE,
    finding_event_key varchar(64) NOT NULL CONSTRAINT finding_event_key_key UNIQUE,
    finding_event_target varchar(128) NOT NULL,
    finding_event_finding varchar(2048) NOT NULL,
    finding_event_resource varchar(2048) NOT NULL,
    finding_event_rule varchar(128) NOT NULL,
    finding_event_kind health_monitor.event_kind NOT NULL,
    finding_event_severity health_monitor.severity NOT NULL,
    finding_event_expected health_monitor.expected NOT NULL,
    finding_event_confidence health_monitor.confidence NOT NULL,
    finding_event_observed_at timestamptz NOT NULL,
    finding_event_at timestamptz NOT NULL,
    finding_event_stale boolean NOT NULL,
    finding_event_evidence text[] NOT NULL,
    CONSTRAINT finding_event_id_v7 CHECK (uuid_extract_version(finding_event_id) IS NOT DISTINCT FROM 7),
    CONSTRAINT finding_event_key_format CHECK (finding_event_key ~ '^[0-9a-f]{64}$'),
    CONSTRAINT finding_event_target_nonempty CHECK (char_length(finding_event_target) > 0),
    CONSTRAINT finding_event_finding_nonempty CHECK (char_length(finding_event_finding) > 0),
    CONSTRAINT finding_event_resource_nonempty CHECK (char_length(finding_event_resource) > 0),
    CONSTRAINT finding_event_rule_nonempty CHECK (char_length(finding_event_rule) > 0),
    CONSTRAINT finding_event_evidence_count CHECK (cardinality(finding_event_evidence) <= 64),
    CONSTRAINT finding_event_evidence_nulls CHECK (array_position(finding_event_evidence, NULL) IS NULL)
);
CREATE FUNCTION health_monitor.valid_evidence(values_to_check text[]) RETURNS boolean LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
    SELECT coalesce(bool_and(char_length(value) BETWEEN 1 AND 1024), true) FROM unnest(values_to_check) AS value;
$$;
ALTER TABLE health_monitor.finding_event ADD CONSTRAINT finding_event_evidence_lengths CHECK (health_monitor.valid_evidence(finding_event_evidence));
CREATE INDEX finding_event_configuration_idx ON health_monitor.finding_event (finding_event_configuration_id);
CREATE INDEX finding_event_at_id_idx ON health_monitor.finding_event (finding_event_at DESC, finding_event_id DESC);
CREATE INDEX finding_event_target_at_idx ON health_monitor.finding_event (finding_event_target, finding_event_at DESC);
CREATE INDEX finding_event_resource_at_idx ON health_monitor.finding_event (finding_event_resource, finding_event_at DESC);
CREATE INDEX finding_event_finding_at_idx ON health_monitor.finding_event (finding_event_finding, finding_event_at DESC);
CREATE INDEX finding_event_kind_at_idx ON health_monitor.finding_event (finding_event_kind, finding_event_at DESC);
CREATE INDEX finding_event_severity_at_idx ON health_monitor.finding_event (finding_event_severity, finding_event_at DESC);
CREATE INDEX finding_event_rule_at_idx ON health_monitor.finding_event (finding_event_rule, finding_event_at DESC);

CREATE TABLE health_monitor.history_gap (
    history_gap_id uuid DEFAULT uuidv7() CONSTRAINT history_gap_pkey PRIMARY KEY,
    history_gap_configuration_id uuid NOT NULL CONSTRAINT history_gap_configuration_fkey REFERENCES health_monitor.configuration(configuration_id) ON DELETE CASCADE,
    history_gap_key varchar(64) NOT NULL CONSTRAINT history_gap_key_key UNIQUE,
    history_gap_at timestamptz NOT NULL,
    history_gap_events bigint NOT NULL,
    history_gap_runs bigint NOT NULL,
    CONSTRAINT history_gap_id_v7 CHECK (uuid_extract_version(history_gap_id) IS NOT DISTINCT FROM 7),
    CONSTRAINT history_gap_key_format CHECK (history_gap_key ~ '^[0-9a-f]{64}$'),
    CONSTRAINT history_gap_events_nonnegative CHECK (history_gap_events >= 0),
    CONSTRAINT history_gap_runs_nonnegative CHECK (history_gap_runs >= 0)
);
CREATE INDEX history_gap_configuration_idx ON health_monitor.history_gap (history_gap_configuration_id);
CREATE INDEX history_gap_at_id_idx ON health_monitor.history_gap (history_gap_at DESC, history_gap_id DESC);
