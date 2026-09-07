//! Server-side billing aggregation keeps charts independent of visible breakdown pages.
use crate::{
    History,
    cost_rows::CostProvider,
    cost_schema::{cost_daily as d, cost_partition as p, cost_source as s},
    error::Error,
};
use chrono::NaiveDate;
use diesel::AggregateExpressionMethods;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use monitor_costs::{
    aggregate::{Bucket, Contributor, View},
    config::Config,
    model::Amount,
    query::{Filter, Group, Measure, Period},
};
use rust_decimal::Decimal;
use crate::cost_cursor::{decode_cursor, encode_cursor, hex_digest};

impl History {
    pub async fn cost_view(&self, filter: &Filter, config: &Config) -> Result<View, Error> {
        let full = filter.period().map_err(|_| Error::Record)?;
        let period = match filter.day {
            Some(day) => Period {
                from: day,
                to: day.succ_opt().ok_or(Error::Record)?,
            },
            None => full,
        };
        if filter.measure != Measure::Billed {
            return Err(Error::Record);
        }
        let currency = filter.currency.as_deref().unwrap_or("USD");
        let sources = crate::cost_coverage::statuses(config, filter, self.cost_status().await?);
        let revision = hex_digest(serde_json::to_vec(&sources).map_err(|_| Error::Record)?);
        if filter.revision.as_ref().is_some_and(|r| r != &revision) {
            return Err(Error::Revision);
        }
        let mut normalized = filter.clone();
        normalized.cursor = None;
        normalized.revision = None;
        let fingerprint = hex_digest(serde_json::to_vec(&normalized).map_err(|_| Error::Record)?);
        let offset = decode_cursor(filter.cursor.as_deref(), &revision, &fingerprint)?;
        let limit = usize::from(filter.limit.unwrap_or(50));
        let previous = Period {
            from: period.from - (period.to - period.from),
            to: period.from,
        };
        let target_scopes: Vec<_> = config
            .scope_targets
            .iter()
            .filter(|m| filter.target.as_ref().is_some_and(|t| t == &m.target))
            .map(|m| m.scope.clone())
            .collect();
        let source_names: Vec<_> = config.sources.iter().map(|s| s.id.clone()).collect();
        let mut c = self.cost_read_pool.get().await.map_err(|_| Error::Pool)?;
        let mut buckets: Vec<Bucket> = Vec::new();
        let mut breakdown: Vec<Contributor> = Vec::new();
        let contributor_count: i64;
        // Each expansion has a statically typed GROUP BY and uses the same scope predicates.
        macro_rules! filter_query {
            ($query:expr, $from:expr, $dimension:expr) => {{
                let mut query = $query
                    .filter(d::cost_daily_day.ge($from))
                    .filter(d::cost_daily_day.lt(period.to))
                    .filter(d::cost_daily_currency.eq(currency))
                    .filter(s::cost_source_name.eq_any(&source_names));
                if let Some(provider) = filter.provider {
                    query = query.filter(s::cost_source_provider.eq(CostProvider::from(provider)));
                }
                if filter.target.is_some() {
                    query = query.filter(d::cost_daily_scope.eq_any(&target_scopes));
                }
                if let Some(scope) = &filter.scope {
                    query = query.filter(d::cost_daily_scope.eq(scope));
                }
                if let Some(product) = &filter.product {
                    query = query.filter(d::cost_daily_product.eq(product));
                }
                if let Some(region) = &filter.region {
                    query = query.filter(d::cost_daily_region.eq(region));
                }
                if let Some(category) = &filter.category {
                    query = query.filter(d::cost_daily_category.eq(category));
                }
                if let Some(resource) = &filter.resource {
                    query = query.filter(d::cost_daily_resource.eq(resource));
                }
                if let Some(key) = &filter.contributor {
                    query = query.filter($dimension.eq(key));
                }
                if let Some(q) = &filter.q {
                    let pattern = format!(
                        "%{}%",
                        q.replace('\\', "\\\\")
                            .replace('%', "\\%")
                            .replace('_', "\\_")
                    );
                    query = query.filter($dimension.ilike(pattern));
                }
                query
            }};
        }
        macro_rules! table {
            () => {
                d::table
                    .inner_join(
                        p::table.on(p::cost_partition_import_id
                            .eq(d::cost_daily_import_id)
                            .and(p::cost_partition_day.eq(d::cost_daily_day))),
                    )
                    .inner_join(s::table.on(s::cost_source_id.eq(p::cost_partition_source_id)))
            };
        }
        macro_rules! aggregate {
            ($dimension:expr, $group:expr) => {{
                let query = table!()
                    .group_by($group)
                    .select(($dimension, diesel::dsl::sum(d::cost_daily_billed)))
                    .into_boxed::<diesel::pg::Pg>();
                let rows: Vec<(String, Option<Decimal>)> =
                    filter_query!(query, period.from, $dimension)
                        .order((
                            diesel::dsl::sum(d::cost_daily_billed).desc(),
                            $dimension.asc(),
                        ))
                        .offset(offset as i64)
                        .limit((limit + 1) as i64)
                        .load(&mut c)
                        .await?;
                for (key, amount) in rows {
                    breakdown.push(Contributor {
                        key,
                        amount: Amount::from_decimal(amount.unwrap_or(Decimal::ZERO))
                            .map_err(|_| Error::Record)?,
                        previous: None,
                    });
                }
                let count = table!()
                    .select(diesel::dsl::count($dimension).aggregate_distinct())
                    .into_boxed::<diesel::pg::Pg>();
                contributor_count = filter_query!(count, period.from, $dimension)
                    .get_result(&mut c)
                    .await?;
                let query = table!()
                    .group_by($group)
                    .select(($dimension, diesel::dsl::sum(d::cost_daily_billed)))
                    .into_boxed::<diesel::pg::Pg>();
                let top: Vec<(String, Option<Decimal>)> =
                    filter_query!(query, period.from, $dimension)
                        .order((
                            diesel::dsl::sum(d::cost_daily_billed).desc(),
                            $dimension.asc(),
                        ))
                        .limit(7)
                        .load(&mut c)
                        .await?;
                let keys: Vec<_> = top.into_iter().map(|(key, _)| key).collect();
                let query = table!()
                    .group_by((d::cost_daily_day, $group))
                    .select((
                        d::cost_daily_day,
                        $dimension,
                        diesel::dsl::sum(d::cost_daily_billed),
                    ))
                    .into_boxed::<diesel::pg::Pg>();
                let rows: Vec<(NaiveDate, String, Option<Decimal>)> =
                    filter_query!(query, previous.from, $dimension)
                        .filter($dimension.eq_any(&keys))
                        .limit(6401)
                        .load(&mut c)
                        .await?;
                if rows.len() > 6400 {
                    return Err(Error::Record);
                }
                for (day, key, amount) in rows {
                    buckets.push(Bucket {
                        day,
                        key,
                        amount: Amount::from_decimal(amount.unwrap_or(Decimal::ZERO))
                            .map_err(|_| Error::Record)?,
                    });
                }
                let query = table!()
                    .group_by(d::cost_daily_day)
                    .select((d::cost_daily_day, diesel::dsl::sum(d::cost_daily_billed)))
                    .into_boxed::<diesel::pg::Pg>();
                let other: Vec<(NaiveDate, Option<Decimal>)> =
                    filter_query!(query, previous.from, $dimension)
                        .filter($dimension.ne_all(&keys))
                        .limit(801)
                        .load(&mut c)
                        .await?;
                if other.len() > 800 {
                    return Err(Error::Record);
                }
                for (day, amount) in other {
                    buckets.push(Bucket {
                        day,
                        key: "__other__".into(),
                        amount: Amount::from_decimal(amount.unwrap_or(Decimal::ZERO))
                            .map_err(|_| Error::Record)?,
                    });
                }
            }};
        }
        match filter.group {
            Group::Provider => {
                aggregate!(
                    s::cost_source_provider.cast::<diesel::sql_types::Text>(),
                    s::cost_source_provider
                );
            }
            Group::Product => {
                aggregate!(d::cost_daily_product, d::cost_daily_product);
            }
            Group::Scope | Group::Target => {
                aggregate!(d::cost_daily_scope, d::cost_daily_scope);
            }
            Group::Region => {
                aggregate!(d::cost_daily_region, d::cost_daily_region);
            }
            Group::Category => {
                aggregate!(d::cost_daily_category, d::cost_daily_category);
            }
            Group::Resource => {
                aggregate!(d::cost_daily_resource, d::cost_daily_resource);
            }
        }
        drop(c);
        // Do not mix new partitions into a response selected against an older publication.
        let after = crate::cost_coverage::statuses(config, filter, self.cost_status().await?);
        if serde_json::to_vec(&sources).ok() != serde_json::to_vec(&after).ok() {
            return Err(Error::Revision);
        }
        let has_data = buckets.iter().any(|b| b.day >= period.from && b.day < period.to);
        let previous_complete = period.to <= chrono::Utc::now().date_naive() && self.cost_covered(&sources, previous).await? && self.cost_covered(&sources, period).await?;
        let total = if has_data {
            Some(monitor_costs::aggregate::total(&buckets, period).map_err(|_| Error::Record)?)
        } else {
            None
        };
        let previous_total = if previous_complete {
            Some(monitor_costs::aggregate::total(&buckets, previous).map_err(|_| Error::Record)?)
        } else {
            None
        };
        let series = monitor_costs::aggregate::series(
            &buckets,
            period,
            filter.granularity,
            previous_complete,
        )
        .map_err(|_| Error::Record)?;
        let next_cursor = if breakdown.len() > limit {
            Some(encode_cursor(offset + limit, &revision, &fingerprint)?)
        } else {
            None
        };
        breakdown.truncate(limit);
        Ok(View {
            enabled: true,
            revision,
            period,
            currency: currency.into(),
            measure: filter.measure,
            group: filter.group,
            granularity: filter.granularity,
            total,
            previous_total,
            complete: false,
            sources,
            series,
            breakdown,
            next_cursor,
            contributor_count: contributor_count as usize,
        })
    }
}
