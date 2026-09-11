//! Typed SQL expressions shared by billing reads; raw SQL stays confined to migrations.
use crate::{cost_schema::cost_daily as d, error::Error};
use bigdecimal::BigDecimal as Decimal;
use monitor_costs::model::Amount;

// PostgreSQL CONCAT gives null dimensions a distinct opaque API key; no sentinel is stored.
diesel::define_sql_function! { #[sql_name = "concat"] fn group_key(prefix: diesel::sql_types::Text, value: diesel::sql_types::Nullable<diesel::sql_types::Text>) -> diesel::sql_types::Text; }
diesel::define_sql_function! { #[sql_name = "coalesce"] fn coalesce_numeric(value: diesel::sql_types::Nullable<diesel::sql_types::Numeric>, fallback: diesel::sql_types::Numeric) -> diesel::sql_types::Numeric; }

/// Effective spend per row; rows imported before adapters reported credits fall back to the
/// list price, so they contribute zero credits instead of hiding the whole day.
pub(crate) fn net() -> coalesce_numeric<d::cost_daily_effective, d::cost_daily_billed> {
    coalesce_numeric(d::cost_daily_effective, d::cost_daily_billed)
}
/// Credits are the signed gap between effective and list-price sums, exact in NUMERIC.
pub(crate) fn credits(billed: &Option<Decimal>, net: &Option<Decimal>) -> Result<Amount, Error> {
    let zero = Decimal::from(0);
    Amount::from_decimal(net.as_ref().unwrap_or(&zero) - billed.as_ref().unwrap_or(&zero))
        .map_err(|_| Error::Record)
}
