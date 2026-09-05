//! Hand-maintained Diesel schema for service-owned monitoring history.
pub mod sql_types {
    #[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
    #[diesel(postgres_type(name = "event_kind", schema = "health_monitor"))]
    pub struct EventKind;
    #[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
    #[diesel(postgres_type(name = "severity", schema = "health_monitor"))]
    pub struct Severity;
    #[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
    #[diesel(postgres_type(name = "expected", schema = "health_monitor"))]
    pub struct Expected;
    #[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
    #[diesel(postgres_type(name = "confidence", schema = "health_monitor"))]
    pub struct Confidence;
    #[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
    #[diesel(postgres_type(name = "check_kind", schema = "health_monitor"))]
    pub struct CheckKind;
}
diesel::table! {
    health_monitor.configuration (configuration_id) {
        configuration_id -> Uuid,
        configuration_revision -> Varchar,
        configuration_seen_at -> Timestamptz,
    }
}
diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::*;
    health_monitor.check_run (check_run_id) {
        check_run_id -> Uuid,
        check_run_configuration_id -> Uuid,
        check_run_key -> Varchar,
        check_run_target -> Varchar,
        check_run_check -> CheckKind,
        check_run_started_at -> Timestamptz,
        check_run_finished_at -> Timestamptz,
        check_run_complete -> Bool,
        check_run_observations -> Int8,
        check_run_required_failures -> Int8,
    }
}
diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::*;
    health_monitor.finding_event (finding_event_id) {
        finding_event_id -> Uuid,
        finding_event_configuration_id -> Uuid,
        finding_event_key -> Varchar,
        finding_event_target -> Varchar,
        finding_event_finding -> Varchar,
        finding_event_resource -> Varchar,
        finding_event_rule -> Varchar,
        finding_event_kind -> EventKind,
        finding_event_severity -> Severity,
        finding_event_expected -> Expected,
        finding_event_confidence -> Confidence,
        finding_event_observed_at -> Timestamptz,
        finding_event_at -> Timestamptz,
        finding_event_stale -> Bool,
        finding_event_evidence -> Array<Text>,
    }
}
diesel::table! {
    health_monitor.history_gap (history_gap_id) {
        history_gap_id -> Uuid,
        history_gap_configuration_id -> Uuid,
        history_gap_key -> Varchar,
        history_gap_at -> Timestamptz,
        history_gap_events -> Int8,
        history_gap_runs -> Int8,
    }
}
diesel::joinable!(check_run -> configuration (check_run_configuration_id));
diesel::joinable!(finding_event -> configuration (finding_event_configuration_id));
diesel::joinable!(history_gap -> configuration (history_gap_configuration_id));
diesel::allow_tables_to_appear_in_same_query!(configuration, check_run, finding_event, history_gap);
