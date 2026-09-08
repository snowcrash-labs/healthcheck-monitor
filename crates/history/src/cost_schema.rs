//! Typed schema for atomic billing partitions; exports remain the source of authority.
pub mod sql_types {
    #[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId)]
    #[diesel(postgres_type(name = "cost_provider", schema = "health_monitor"))]
    pub struct CostProvider;
}
diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::*;
    health_monitor.cost_source (cost_source_id) {
        cost_source_id -> Uuid,
        cost_source_name -> Varchar,
        cost_source_provider -> CostProvider,
        cost_source_scope -> Varchar,
        cost_source_imported_at -> Nullable<Timestamptz>,
        cost_source_revision -> Nullable<Uuid>,
        cost_source_fault -> Nullable<Varchar>,
    }
}
diesel::table! {
    health_monitor.cost_import (cost_import_id) {
        cost_import_id -> Uuid,
        cost_import_source_id -> Uuid,
        cost_import_started_at -> Timestamptz,
        cost_import_published_at -> Nullable<Timestamptz>,
        cost_import_from -> Date,
        cost_import_to -> Date,
        cost_import_reserved_bytes -> Int8,
        cost_import_billed_bytes -> Nullable<Int8>,
        cost_import_attempted_at -> Nullable<Timestamptz>,
    }
}
diesel::table! {
    health_monitor.cost_partition (cost_partition_id) {
        cost_partition_id -> Uuid,
        cost_partition_source_id -> Uuid,
        cost_partition_day -> Date,
        cost_partition_import_id -> Uuid,
    }
}
diesel::table! {
    health_monitor.cost_daily (cost_daily_id) {
        cost_daily_target -> Nullable<Varchar>,
        cost_daily_id -> Uuid,
        cost_daily_import_id -> Uuid,
        cost_daily_key -> Varchar,
        cost_daily_day -> Date,
        cost_daily_invoice_month -> Nullable<Varchar>,
        cost_daily_scope -> Nullable<Varchar>,
        cost_daily_region -> Nullable<Varchar>,
        cost_daily_product -> Varchar,
        cost_daily_resource -> Nullable<Varchar>,
        cost_daily_category -> Nullable<Varchar>,
        cost_daily_currency -> Varchar,
        cost_daily_billed -> Numeric,
        cost_daily_effective -> Nullable<Numeric>,
    }
}
diesel::allow_tables_to_appear_in_same_query!(cost_source, cost_import, cost_partition, cost_daily);

impl diesel::expression::FallibleCastsTo<diesel::sql_types::Text> for sql_types::CostProvider {}
impl diesel::expression::CastsTo<diesel::sql_types::Text> for sql_types::CostProvider {}

diesel::allow_columns_to_appear_in_same_group_by_clause!(
    cost_source::cost_source_provider,
    cost_daily::cost_daily_day
);
