//! Exact daily aggregates are derived from an identified, replaceable source revision.
use crate::error::Error;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Gcp,
    Aws,
    Azure,
    External,
}
impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gcp => "gcp",
            Self::Aws => "aws",
            Self::Azure => "azure",
            Self::External => "external",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Amount(Decimal);
impl Amount {
    pub fn zero() -> Self {
        Self(Decimal::ZERO)
    }
    pub fn decimal(&self) -> Decimal {
        self.0
    }
    pub fn from_decimal(value: Decimal) -> Result<Self, Error> {
        // NUMERIC(28,9): at most nineteen integral and nine fractional digits.
        if value.scale() > 9
            || value.abs() >= Decimal::from_i128_with_scale(10_000_000_000_000_000_000, 0)
        {
            return Err(Error::Amount);
        }
        Ok(Self(value.normalize()))
    }
    pub fn add(&self, other: &Self) -> Result<Self, Error> {
        self.0
            .checked_add(other.0)
            .ok_or(Error::Amount)
            .and_then(Self::from_decimal)
    }
}
impl TryFrom<String> for Amount {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty()
            || value.len() > 32
            || value.split_once('.').is_some_and(|(_, fraction)| fraction.len() > 9)
            || value
                .bytes()
                .any(|b| !b.is_ascii_digit() && b != b'.' && b != b'-')
        {
            return Err(Error::Amount);
        }
        value
            .parse::<Decimal>()
            .map_err(|_| Error::Amount)
            .and_then(Self::from_decimal)
    }
}
impl From<Amount> for String {
    fn from(value: Amount) -> Self {
        value.0.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Charge {
    pub day: NaiveDate,
    pub invoice_month: String,
    pub provider: Provider,
    pub scope: String,
    pub region: String,
    pub product: String,
    pub resource: String,
    pub category: String,
    pub currency: String,
    pub billed: Amount,
    pub effective: Option<Amount>,
}
impl Charge {
    pub fn validate(&self) -> Result<(), Error> {
        if !currency(&self.currency)
            || self.invoice_month.len() != 6
            || !self.invoice_month.bytes().all(|b| b.is_ascii_digit())
            || !self.invoice_month[4..]
                .parse::<u8>()
                .is_ok_and(|month| (1..=12).contains(&month))
            || self.product.is_empty()
            || self.category.is_empty()
            || self.product.len() > 256
            || self.category.len() > 128
            || self.scope.len() > 128
            || self.region.len() > 128
            || [
                &self.scope,
                &self.region,
                &self.product,
                &self.resource,
                &self.category,
            ]
            .iter()
            .any(|v| v.len() > 2048 || v.chars().any(char::is_control))
        {
            return Err(Error::Record);
        }
        Ok(())
    }
}
pub fn currency(value: &str) -> bool {
    // Explicit supported ISO 4217 billing currencies; additions are reviewed with adapters.
    [
        "USD", "EUR", "GBP", "JPY", "CAD", "AUD", "NZD", "CHF", "INR", "KRW", "SGD", "BRL", "MXN",
        "CNY", "HKD", "TWD", "SEK", "NOK", "DKK", "PLN", "ZAR", "IDR", "MYR", "THB", "TRY", "ILS",
        "AED", "SAR", "CLP", "COP", "PEN",
    ]
    .contains(&value)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceStatus {
    pub id: String,
    pub provider: Provider,
    pub state: String,
    pub imported_at: Option<DateTime<Utc>>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub revision: Option<String>,
    pub fault: Option<String>,
}
