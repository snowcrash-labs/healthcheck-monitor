//! Indexed temporal projections are independent of configuration eviction.
pub mod sql_types {
    #[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
    #[diesel(postgres_type(name = "query_category", schema = "health_monitor"))]
    pub struct QueryCategory;
    #[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
    #[diesel(postgres_type(name = "query_provider", schema = "health_monitor"))]
    pub struct QueryProvider;
}
diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::*;
    use crate::schema::sql_types::{CheckKind, Severity};
    health_monitor.query_record (query_record_id) {
        query_record_id -> Uuid,
        query_record_key -> Varchar,
        query_record_identity -> Varchar,
        query_record_category -> QueryCategory,
        query_record_provider -> QueryProvider,
        query_record_target -> Varchar,
        query_record_scope -> Varchar,
        query_record_check -> Nullable<CheckKind>,
        query_record_resource -> Nullable<Varchar>,
        query_record_region -> Nullable<Varchar>,
        query_record_cluster -> Nullable<Varchar>,
        query_record_namespace -> Nullable<Varchar>,
        query_record_service -> Nullable<Varchar>,
        query_record_hostname -> Nullable<Varchar>,
        query_record_severity -> Nullable<Severity>,
        query_record_state -> Nullable<Varchar>,
        query_record_from -> Timestamptz,
        query_record_to -> Timestamptz,
        query_record_closed_at -> Nullable<Timestamptz>,
        query_record_written_at -> Timestamptz,
        query_record_payload -> Jsonb,
    }
}
diesel::table! {
    health_monitor.query_watermark (query_watermark_id) {
        query_watermark_id -> Uuid,
        query_watermark_name -> Varchar,
        query_watermark_since -> Timestamptz,
        query_watermark_evicted_through -> Nullable<Timestamptz>,
        query_watermark_persisted_at -> Nullable<Timestamptz>,
    }
}
