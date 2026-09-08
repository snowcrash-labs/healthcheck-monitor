//! Server-side billing aggregation keeps charts independent of visible breakdown pages.
use crate::cost_cursor::{decode_cursor, hex_digest};
use crate::{
    History,
    cost_rows::CostProvider,
    cost_schema::{cost_daily as d, cost_partition as p, cost_source as s},
    error::Error,
};
use bigdecimal::BigDecimal as Decimal;
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
        normalized.from = Some(full.from);
        normalized.to = Some(full.to);
        normalized.cursor = None;
        normalized.revision = None;
        let fingerprint = hex_digest(serde_json::to_vec(&normalized).map_err(|_| Error::Record)?);
        let boundary = decode_cursor(filter.cursor.as_deref(), &revision, &fingerprint)?;
        let limit = usize::from(filter.limit.unwrap_or(50));
        let previous = Period {
            from: period.from - (period.to - period.from),
            to: period.from,
        };
        let source_names: Vec<_> = config.sources.iter().map(|s| s.id.clone()).collect();
        let mut c = self.cost_read_pool.get().await.map_err(|_| Error::Pool)?;
        let mut buckets: Vec<Bucket> = Vec::new();
        let mut breakdown: Vec<Contributor> = Vec::new();
        let contributor_count: i64;
        let mut excluded: Vec<String> = Vec::new();
        // Each expansion has a statically typed GROUP BY and uses the same scope predicates.
        macro_rules! filter_query {
            ($query:expr, $from:expr, $to:expr, $dimension:expr, $search:expr) => {{
                let mut query = $query
                    .filter(d::cost_daily_day.ge($from))
                    .filter(d::cost_daily_day.lt($to))
                    .filter(d::cost_daily_currency.eq(currency))
                    .filter(s::cost_source_name.eq_any(&source_names));
                if let Some(provider) = filter.provider {
                    query = query.filter(s::cost_source_provider.eq(CostProvider::from(provider)));
                }
                if let Some(target) = &filter.target {
                    query = query.filter(d::cost_daily_target.eq(target));
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
                query = query.filter($dimension.ne_all(&excluded));
                if let Some(key) = &filter.contributor
                    && key != "__other__"
                {
                    query = query.filter($dimension.eq(key));
                }
                if $search && let Some(q) = &filter.q {
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
                // Other keeps the original range's seven leading contributors excluded while searching.
                if filter.contributor.as_deref() == Some("__other__") {
                    let query = table!().group_by($group)
                        .select($dimension).into_boxed::<diesel::pg::Pg>();
                    excluded = filter_query!(query, full.from, full.to, $dimension, false)
                        .order((diesel::dsl::sum(d::cost_daily_billed).desc(), $dimension.asc()))
                        .limit(7).load(&mut c).await?;
                }
                let query = table!()
                    .group_by($group)
                    .select(($dimension, diesel::dsl::sum(d::cost_daily_billed)))
                    .into_boxed::<diesel::pg::Pg>();
                let query = match &boundary {
                    Some(boundary) => query.having(
                        diesel::dsl::sum(d::cost_daily_billed)
                            .lt(boundary.amount.decimal())
                            .or(diesel::dsl::sum(d::cost_daily_billed)
                                .eq(boundary.amount.decimal())
                                .and($dimension.gt(&boundary.key))),
                    ),
                    None => query,
                };
                let rows: Vec<(String, Option<Decimal>)> =
                    filter_query!(query, period.from, period.to, $dimension, true)
                        .order((
                            diesel::dsl::sum(d::cost_daily_billed).desc(),
                            $dimension.asc(),
                        ))
                        .limit((limit + 1) as i64)
                        .load(&mut c)
                        .await?;
                for (key, amount) in rows {
                    breakdown.push(Contributor {
                        key,
                        amount: Amount::from_decimal(amount.unwrap_or(Decimal::from(0)))
                            .map_err(|_| Error::Record)?,
                        previous: None,
                    });
                }
                let page_keys: Vec<_> = breakdown.iter().map(|row| &row.key).collect();
                let query = table!().group_by($group)
                    .select(($dimension, diesel::dsl::sum(d::cost_daily_billed)))
                    .into_boxed::<diesel::pg::Pg>();
                let prior: Vec<(String, Option<Decimal>)> =
                    filter_query!(query, previous.from, previous.to, $dimension, true)
                        .filter($dimension.eq_any(&page_keys)).limit((limit + 1) as i64)
                        .load(&mut c).await?;
                let mut prior: std::collections::BTreeMap<_, _> = prior.into_iter().collect();
                for row in &mut breakdown {
                    row.previous = Some(Amount::from_decimal(prior.remove(&row.key).flatten()
                        .unwrap_or(Decimal::from(0))).map_err(|_| Error::Record)?);
                }
                let count = table!()
                    .select(diesel::dsl::count($dimension).aggregate_distinct())
                    .into_boxed::<diesel::pg::Pg>();
                contributor_count = filter_query!(count, period.from, period.to, $dimension, true)
                    .get_result(&mut c)
                    .await?;
                let query = table!()
                    .group_by($group)
                    .select(($dimension, diesel::dsl::sum(d::cost_daily_billed)))
                    .into_boxed::<diesel::pg::Pg>();
                let top: Vec<(String, Option<Decimal>)> =
                    filter_query!(query, period.from, period.to, $dimension, true)
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
                    filter_query!(query, previous.from, period.to, $dimension, true)
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
                        amount: Amount::from_decimal(amount.unwrap_or(Decimal::from(0)))
                            .map_err(|_| Error::Record)?,
                    });
                }
                let query = table!()
                    .group_by(d::cost_daily_day)
                    .select((d::cost_daily_day, diesel::dsl::sum(d::cost_daily_billed)))
                    .into_boxed::<diesel::pg::Pg>();
                let other: Vec<(NaiveDate, Option<Decimal>)> =
                    filter_query!(query, previous.from, period.to, $dimension, true)
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
                        amount: Amount::from_decimal(amount.unwrap_or(Decimal::from(0)))
                            .map_err(|_| Error::Record)?,
                    });
                }
            }};
        }
        macro_rules! column {
            ($column:expr) => {
                aggregate!(group_key("v:", $column.nullable()), $column)
            };
        }
        match filter.group {
            Group::Provider => aggregate!(
                group_key(
                    "v:",
                    s::cost_source_provider
                        .cast::<diesel::sql_types::Text>()
                        .nullable()
                ),
                s::cost_source_provider
            ),
            Group::Product => column!(d::cost_daily_product),
            Group::Scope => column!(d::cost_daily_scope),
            Group::Target => column!(d::cost_daily_target),
            Group::Region => column!(d::cost_daily_region),
            Group::Category => column!(d::cost_daily_category),
            Group::Resource => column!(d::cost_daily_resource),
        }
        drop(c);
        self.cost_response(
            filter,
            config,
            sources,
            crate::cost_response::Draft {
                buckets,
                breakdown,
                contributor_count,
                period,
                previous,
                revision,
                fingerprint,
                limit,
            },
        )
        .await
    }
}

// PostgreSQL CONCAT gives null dimensions a distinct opaque API key; no sentinel is stored.
diesel::define_sql_function! { #[sql_name = "concat"] fn group_key(prefix: diesel::sql_types::Text, value: diesel::sql_types::Nullable<diesel::sql_types::Text>) -> diesel::sql_types::Text; }
