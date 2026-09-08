//! Billing ranges are separate from the shorter diagnostic-history query window.
use crate::{
    error::Error,
    model::{Provider, currency},
};
use chrono::{Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    #[default]
    Provider,
    Product,
    Scope,
    Region,
    Category,
    Resource,
    Target,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    #[default]
    Billed,
    Effective,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Granularity {
    #[default]
    Daily,
    Monthly,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Filter {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub currency: Option<String>,
    pub measure: Measure,
    pub group: Group,
    pub granularity: Granularity,
    pub target: Option<String>,
    pub provider: Option<Provider>,
    pub scope: Option<String>,
    pub product: Option<String>,
    pub region: Option<String>,
    pub category: Option<String>,
    pub resource: Option<String>,
    pub contributor: Option<String>,
    pub day: Option<NaiveDate>,
    pub q: Option<String>,
    pub cursor: Option<String>,
    pub revision: Option<String>,
    pub limit: Option<u16>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Period {
    pub from: NaiveDate,
    pub to: NaiveDate,
}
impl Filter {
    pub fn period(&self) -> Result<Period, Error> {
        let to = self.to.unwrap_or_else(|| Utc::now().date_naive());
        let from = self.from.unwrap_or(to - chrono::Duration::days(30));
        if from.year() < 1970
            || to <= from
            || (to - from).num_days() > 400
            || to > Utc::now().date_naive() + chrono::Duration::days(1)
            || self.currency.as_ref().is_some_and(|c| !currency(c))
            || self.limit.is_some_and(|n| !(1..=100).contains(&n))
            || [
                &self.target,
                &self.scope,
                &self.product,
                &self.region,
                &self.category,
                &self.resource,
                &self.contributor,
                &self.q,
            ]
            .iter()
            .any(|v| {
                v.as_ref().is_some_and(|s| {
                    s.is_empty() || s.len() > 2048 || s.chars().any(char::is_control)
                })
            })
            || self.cursor.as_ref().is_some_and(|v| v.len() > 8192)
            || self.revision.as_ref().is_some_and(|v| v.len() > 128)
            || self.day.is_some_and(|d| d < from || d >= to)
        {
            return Err(Error::Query);
        }
        Ok(Period { from, to })
    }
}
