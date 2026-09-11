//! Exact daily aggregates are derived from an identified, replaceable source revision.
use crate::error::Error;
use bigdecimal::BigDecimal as Decimal;
use chrono::{DateTime, NaiveDate, Utc};
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
    /// Normalize bounded scientific decimal tokens supplied by provider query APIs.
    pub fn provider(value: &str) -> Result<Self, Error> {
        if value.len() > 64
            || value.is_empty()
            || value
                .bytes()
                .any(|b| !b.is_ascii_digit() && !b".+-eE".contains(&b))
        {
            return Err(Error::Amount);
        }
        if let Some((_, exponent)) = value.split_once(['e', 'E'])
            && !exponent
                .parse::<i32>()
                .is_ok_and(|e| e.unsigned_abs() <= 38)
        {
            return Err(Error::Amount);
        }
        Self::from_decimal(value.parse().map_err(|_| Error::Amount)?)
    }
    pub fn zero() -> Self {
        Self(Decimal::from(0))
    }
    pub fn decimal(&self) -> Decimal {
        self.0.clone()
    }
    pub fn from_decimal(value: Decimal) -> Result<Self, Error> {
        // NUMERIC(58,38) preserves submicro usage charges and twenty integer digits.
        if value.fractional_digit_count() > 38
            || value.abs() >= Decimal::from(10_000_000_000_000_000_000u64) * 10
        {
            return Err(Error::Amount);
        }
        Ok(Self(value.normalized()))
    }
    pub fn add(&self, other: &Self) -> Result<Self, Error> {
        Self::from_decimal(&self.0 + &other.0)
    }
}
impl TryFrom<String> for Amount {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        if value.is_empty()
            || value.len() > 60
            || value
                .split_once('.')
                .is_some_and(|(_, fraction)| fraction.len() > 38)
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
        value.0.to_plain_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Charge {
    #[serde(default)]
    pub target: Option<String>,
    pub day: NaiveDate,
    pub invoice_month: Option<String>,
    pub provider: Provider,
    pub scope: Option<String>,
    pub region: Option<String>,
    pub product: String,
    pub resource: Option<String>,
    pub category: Option<String>,
    pub currency: String,
    /// List-price spend before credits; the amount a provider would charge without promotions.
    pub billed: Amount,
    /// Spend after every credit, when the adapter can report it. `None` means credits are unknown
    /// for this row and the dashboard shows no credit portion rather than a zero credit.
    pub effective: Option<Amount>,
}
impl Charge {
    pub fn validate(&self) -> Result<(), Error> {
        if self
            .target
            .as_ref()
            .is_some_and(|v| !crate::config::identifier(v))
            || !currency(&self.currency)
            || self.product.is_empty()
            || self.product.len() > 256
            || self.product.chars().any(char::is_control)
        {
            return Err(Error::Record);
        }
        if self.invoice_month.as_ref().is_some_and(|month| {
            month.len() != 6
                || !month.bytes().all(|b| b.is_ascii_digit())
                || !month[4..]
                    .parse::<u8>()
                    .is_ok_and(|n| (1..=12).contains(&n))
        }) {
            return Err(Error::Record);
        }
        for (value, limit) in [
            (&self.scope, 128),
            (&self.region, 128),
            (&self.resource, 2048),
            (&self.category, 128),
        ] {
            if value
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > limit || v.chars().any(char::is_control))
            {
                return Err(Error::Record);
            }
        }
        Ok(())
    }
}
/// Provider empty dimensions represent an explicit absence after projection.
pub fn optional(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
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
