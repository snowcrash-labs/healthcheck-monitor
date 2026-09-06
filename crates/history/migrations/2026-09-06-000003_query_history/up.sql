CREATE TYPE health_monitor.query_category AS ENUM ('finding', 'diagnostic', 'check', 'release');
CREATE TYPE health_monitor.query_provider AS ENUM ('gcp', 'aws', 'azure', 'kubernetes', 'github', 'edge', 'nats');

CREATE TABLE health_monitor.query_record (
    query_record_id uuid DEFAULT uuidv7() CONSTRAINT query_record_pkey PRIMARY KEY,
    query_record_key varchar(64) NOT NULL CONSTRAINT query_record_key_key UNIQUE,
    query_record_identity varchar(2048) NOT NULL,
    query_record_category health_monitor.query_category NOT NULL,
    query_record_provider health_monitor.query_provider NOT NULL,
    query_record_target varchar(128) NOT NULL,
    query_record_scope varchar(2048) NOT NULL,
    query_record_check health_monitor.check_kind,
    query_record_resource varchar(2048),
    query_record_region varchar(2048),
    query_record_cluster varchar(2048),
    query_record_namespace varchar(2048),
    query_record_service varchar(2048),
    query_record_hostname varchar(253),
    query_record_severity health_monitor.severity,
    query_record_state varchar(16),
    query_record_from timestamptz NOT NULL,
    query_record_to timestamptz NOT NULL,
    query_record_closed_at timestamptz,
    query_record_written_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    query_record_payload jsonb NOT NULL,
    CONSTRAINT query_record_id_v7 CHECK (uuid_extract_version(query_record_id) IS NOT DISTINCT FROM 7),
    CONSTRAINT query_record_key_format CHECK (query_record_key ~ '^[0-9a-f]{64}$'),
    CONSTRAINT query_record_identity_nonempty CHECK (char_length(query_record_identity) > 0),
    CONSTRAINT query_record_target_nonempty CHECK (char_length(query_record_target) > 0),
    CONSTRAINT query_record_scope_nonempty CHECK (char_length(query_record_scope) > 0),
    CONSTRAINT query_record_time_order CHECK (query_record_to >= query_record_from),
    CONSTRAINT query_record_state_valid CHECK (query_record_state IS NULL OR query_record_state IN ('active', 'recovered', 'removed')),
    CONSTRAINT query_record_payload_bound CHECK (octet_length(query_record_payload::text) <= 32768),
    CONSTRAINT query_record_payload_object CHECK (jsonb_typeof(query_record_payload) = 'object'),
    CONSTRAINT query_record_payload_kind CHECK (query_record_payload->'details'->>'kind' = query_record_category::text),
    CONSTRAINT query_record_payload_fields CHECK (query_record_payload - ARRAY['id','identity','scope','location','check','resource','observed_at','last_observed_at','valid_until','closed_at','stale','details'] = '{}'::jsonb)
);
CREATE INDEX query_record_time_id_idx ON health_monitor.query_record(query_record_from DESC, query_record_id DESC);
CREATE INDEX query_record_retention_idx ON health_monitor.query_record(query_record_to, query_record_id);
CREATE INDEX query_record_identity_idx ON health_monitor.query_record(query_record_identity, query_record_from DESC);
CREATE INDEX query_record_target_kind_time_idx ON health_monitor.query_record(query_record_target, query_record_category, query_record_from DESC, query_record_id DESC);
CREATE INDEX query_record_scope_kind_time_idx ON health_monitor.query_record(query_record_provider, query_record_scope, query_record_category, query_record_from DESC, query_record_id DESC);
CREATE INDEX query_record_kind_time_idx ON health_monitor.query_record(query_record_category, query_record_from DESC, query_record_id DESC);
CREATE INDEX query_record_check_time_idx ON health_monitor.query_record(query_record_check, query_record_from DESC);
CREATE INDEX query_record_resource_time_idx ON health_monitor.query_record(query_record_resource, query_record_from DESC);
CREATE INDEX query_record_region_idx ON health_monitor.query_record(query_record_region);
CREATE INDEX query_record_cluster_idx ON health_monitor.query_record(query_record_cluster);
CREATE INDEX query_record_namespace_idx ON health_monitor.query_record(query_record_namespace);
CREATE INDEX query_record_service_idx ON health_monitor.query_record(query_record_service);
CREATE INDEX query_record_hostname_idx ON health_monitor.query_record(query_record_hostname);
CREATE INDEX query_record_severity_idx ON health_monitor.query_record(query_record_severity);
CREATE INDEX query_record_state_idx ON health_monitor.query_record(query_record_state);
CREATE INDEX query_record_written_at_idx ON health_monitor.query_record(query_record_written_at);

CREATE TABLE health_monitor.query_watermark (
    query_watermark_id uuid DEFAULT uuidv7() CONSTRAINT query_watermark_pkey PRIMARY KEY,
    query_watermark_name varchar(32) NOT NULL CONSTRAINT query_watermark_name_key UNIQUE,
    query_watermark_since timestamptz NOT NULL DEFAULT clock_timestamp(),
    query_watermark_evicted_through timestamptz,
    query_watermark_persisted_at timestamptz,
    CONSTRAINT query_watermark_id_v7 CHECK (uuid_extract_version(query_watermark_id) IS NOT DISTINCT FROM 7),
    CONSTRAINT query_watermark_name_fixed CHECK (query_watermark_name = 'diagnostics')
);
INSERT INTO health_monitor.query_watermark(query_watermark_name) VALUES ('diagnostics');
