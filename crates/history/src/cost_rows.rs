//! Database rows bind constrained billing dimensions to native UUID and numeric columns.
use crate::{
    cost_schema::{cost_daily as d, cost_source as s},
    error::Error,
    types::Id,
};
use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use monitor_costs::model::{Charge, Provider};
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, diesel_derive_enum::DbEnum)]
#[ExistingTypePath = "crate::cost_schema::sql_types::CostProvider"]
pub enum CostProvider {
    Gcp,
    Aws,
    Azure,
    External,
}
impl From<Provider> for CostProvider {
    fn from(value: Provider) -> Self {
        match value {
            Provider::Gcp => Self::Gcp,
            Provider::Aws => Self::Aws,
            Provider::Azure => Self::Azure,
            Provider::External => Self::External,
        }
    }
}
impl From<CostProvider> for Provider {
    fn from(value: CostProvider) -> Self {
        match value {
            CostProvider::Gcp => Self::Gcp,
            CostProvider::Aws => Self::Aws,
            CostProvider::Azure => Self::Azure,
            CostProvider::External => Self::External,
        }
    }
}
#[derive(Queryable, Selectable)]
#[diesel(table_name = s)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Source {
    pub cost_source_id: Id,
    pub cost_source_name: String,
    pub cost_source_provider: CostProvider,
    pub cost_source_scope: String,
    pub cost_source_imported_at: Option<DateTime<Utc>>,
    pub cost_source_revision: Option<Id>,
    pub cost_source_fault: Option<String>,
}
#[derive(Insertable)]
#[diesel(table_name = d)]
pub struct Daily {
    cost_daily_import_id: Id,
    cost_daily_key: String,
    cost_daily_day: NaiveDate,
    cost_daily_invoice_month: String,
    cost_daily_scope: String,
    cost_daily_region: String,
    cost_daily_product: String,
    cost_daily_resource: String,
    cost_daily_category: String,
    cost_daily_currency: String,
    cost_daily_billed: Decimal,
    cost_daily_effective: Option<Decimal>,
}
impl Daily {
    pub fn new(import: Id, charge: Charge) -> Result<Self, Error> {
        charge.validate().map_err(|_| Error::Record)?;
        let key = serde_json::to_vec(&(
            &charge.day,
            &charge.invoice_month,
            &charge.scope,
            &charge.region,
            &charge.product,
            &charge.resource,
            &charge.category,
            &charge.currency,
        ))
        .map_err(|_| Error::Record)?;
        Ok(Self {
            cost_daily_import_id: import,
            cost_daily_key: hex_digest(&key),
            cost_daily_day: charge.day,
            cost_daily_invoice_month: charge.invoice_month,
            cost_daily_scope: charge.scope,
            cost_daily_region: charge.region,
            cost_daily_product: charge.product,
            cost_daily_resource: charge.resource,
            cost_daily_category: charge.category,
            cost_daily_currency: charge.currency,
            cost_daily_billed: charge.billed.decimal(),
            cost_daily_effective: charge.effective.map(|a| a.decimal()),
        })
    }
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
