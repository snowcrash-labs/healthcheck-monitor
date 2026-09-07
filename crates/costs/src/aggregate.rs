//! Chart reductions preserve all contributors, signed adjustments, and exact totals.
use crate::{
    error::Error,
    model::{Amount, SourceStatus},
    query::{Granularity, Group, Measure, Period},
};
use chrono::{Datelike, NaiveDate};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Bucket {
    pub day: NaiveDate,
    pub key: String,
    pub amount: Amount,
}
#[derive(Debug, Clone, Serialize)]
pub struct Contributor {
    pub key: String,
    pub amount: Amount,
    pub previous: Option<Amount>,
}
#[derive(Debug, Serialize)]
pub struct Point {
    pub date: NaiveDate,
    pub total: Amount,
    pub previous: Option<Amount>,
    pub contributors: Vec<Contributor>,
}
#[derive(Debug, Serialize)]
pub struct View {
    pub enabled: bool,
    pub revision: String,
    pub period: Period,
    pub currency: String,
    pub measure: Measure,
    pub group: Group,
    pub granularity: Granularity,
    pub total: Option<Amount>,
    pub previous_total: Option<Amount>,
    pub complete: bool,
    pub sources: Vec<SourceStatus>,
    pub series: Vec<Point>,
    pub breakdown: Vec<Contributor>,
    pub next_cursor: Option<String>,
    pub contributor_count: usize,
}
/// Convert already bounded database aggregates, never raw provider billing records.
pub fn series(
    rows: &[Bucket],
    period: Period,
    granularity: Granularity,
    previous_complete: bool,
) -> Result<Vec<Point>, Error> {
    let mut days = BTreeMap::<NaiveDate, BTreeMap<String, Amount>>::new();
    let mut prior = BTreeMap::<NaiveDate, Amount>::new();
    let offset = period.to - period.from;
    for row in rows {
        let current = row.day >= period.from;
        let aligned = if current {
            row.day
        } else {
            row.day.checked_add_signed(offset).ok_or(Error::Query)?
        };
        let day = match granularity {
            Granularity::Daily => aligned,
            Granularity::Monthly => aligned.with_day(1).ok_or(Error::Query)?,
        };
        if current {
            let group = days.entry(day).or_default();
            let amount = group.entry(row.key.clone()).or_insert_with(Amount::zero);
            *amount = amount.add(&row.amount)?;
        } else {
            let amount = prior.entry(day).or_insert_with(Amount::zero);
            *amount = amount.add(&row.amount)?;
        }
    }
    days.into_iter()
        .map(|(date, groups)| {
            let mut total = Amount::zero();
            let mut contributors = Vec::new();
            for (key, amount) in groups {
                total = total.add(&amount)?;
                contributors.push(Contributor {
                    key,
                    amount,
                    previous: None,
                });
            }
            Ok(Point {
                date,
                total,
                previous: if previous_complete {
                    Some(prior.remove(&date).unwrap_or_else(Amount::zero))
                } else {
                    None
                },
                contributors,
            })
        })
        .collect()
}
pub fn total(rows: &[Bucket], period: Period) -> Result<Amount, Error> {
    rows.iter()
        .filter(|r| r.day >= period.from && r.day < period.to)
        .try_fold(Amount::zero(), |sum, row| sum.add(&row.amount))
}
