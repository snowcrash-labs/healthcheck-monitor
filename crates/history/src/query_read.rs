//! Indexed scope and time queries over compact, allowlisted diagnostic history.
use crate::{
    enums,
    error::Error,
    pool::History,
    query_records::Stored,
    query_schema::{query_record as r, query_watermark as w},
    types::Id,
};
use chrono::{DateTime, Utc};
use diesel::AggregateExpressionMethods;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use monitor_query::{
    enums::{Category, FindingState, Severity},
    filter::{Filter, Window},
    record::Record,
};

pub(crate) fn selection(
    filter: &Filter,
    window: Window,
    category: Option<Category>,
) -> r::BoxedQuery<'static, diesel::pg::Pg> {
    let mut q = r::table
        .into_boxed()
        .filter(r::query_record_from.lt(window.to))
        .filter(
            r::query_record_to
                .ge(window.from)
                .or(r::query_record_category
                    .eq(enums::QueryCategory::Finding)
                    .and(
                        r::query_record_closed_at
                            .is_null()
                            .or(r::query_record_closed_at.ge(window.from)),
                    )),
        );
    if let Some(category) = category {
        q = q.filter(r::query_record_category.eq(enums::QueryCategory::from(category)));
    }
    macro_rules! text_filter {
        ($value:expr, $column:path) => {
            if let Some(value) = $value {
                q = q.filter($column.eq(value.clone()));
            }
        };
    }
    text_filter!(&filter.target, r::query_record_target);
    text_filter!(&filter.scope, r::query_record_scope);
    text_filter!(&filter.resource, r::query_record_resource);
    text_filter!(&filter.region, r::query_record_region);
    text_filter!(&filter.cluster, r::query_record_cluster);
    text_filter!(&filter.namespace, r::query_record_namespace);
    text_filter!(&filter.service, r::query_record_service);
    text_filter!(&filter.hostname, r::query_record_hostname);
    if let Some(provider) = filter.provider {
        q = q.filter(r::query_record_provider.eq(enums::QueryProvider::from(provider)));
    }
    if let Some(check) = filter.check {
        q = q.filter(r::query_record_check.eq(enums::Check::from(check)));
    }
    if let Some(severity) = filter.severity {
        q = q.filter(r::query_record_severity.eq(enums::Severity::from(severity)));
    }
    if let Some(state) = filter.state {
        q = q.filter(r::query_record_state.eq(state.as_str().to_ascii_lowercase()));
        if state == FindingState::Active {
            q = q.filter(r::query_record_closed_at.is_null());
        }
    }
    if let Some(search) = &filter.q {
        let literal = search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{literal}%");
        q = q.filter(
            r::query_record_identity
                .ilike(pattern.clone())
                .or(r::query_record_payload
                    .retrieve_as_text("details")
                    .ilike(pattern)),
        );
    }
    q
}
impl History {
    pub async fn query_failed_checks(&self, filter: &Filter, window: Window) -> Result<i64, Error> {
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        selection(filter, window, Some(Category::Check))
            .filter(
                r::query_record_payload
                    .retrieve_as_object("details")
                    .retrieve_as_text("complete")
                    .eq("false"),
            )
            .select(diesel::dsl::count(r::query_record_identity).aggregate_distinct())
            .get_result(&mut connection)
            .await
            .map_err(Error::from)
    }
    pub async fn query_page(
        &self,
        filter: &Filter,
        window: Window,
        category: Option<Category>,
        before: Option<(DateTime<Utc>, Id)>,
    ) -> Result<Vec<Record>, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        let limit = filter.limit.unwrap_or(50);
        if !(1..=100).contains(&limit) {
            return Err(Error::Record);
        }
        let mut query = selection(filter, window, category);
        if let Some((at, id)) = before {
            query = query.filter(
                r::query_record_from
                    .lt(at)
                    .or(r::query_record_from.eq(at).and(r::query_record_id.lt(id))),
            );
        }
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        let rows: Vec<Stored> = query
            .order((r::query_record_from.desc(), r::query_record_id.desc()))
            .limit(i64::from(limit) + 1)
            .select(Stored::as_select())
            .load(&mut connection)
            .await?;
        rows.into_iter().map(Stored::decode).collect()
    }
    /// Aggregate in PostgreSQL without walking pages or allocating a full history view.
    pub async fn query_count(
        &self,
        filter: &Filter,
        window: Window,
        category: Category,
        severity: Option<Severity>,
    ) -> Result<i64, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        let mut filter = filter.clone();
        if severity.is_some_and(|requested| {
            filter
                .severity
                .is_some_and(|selected| selected != requested)
        }) {
            return Ok(0);
        }
        if severity.is_some() {
            filter.severity = severity;
        }
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        selection(&filter, window, Some(category))
            .select(diesel::dsl::count(r::query_record_identity).aggregate_distinct())
            .get_result(&mut connection)
            .await
            .map_err(Error::from)
    }
    pub async fn query_availability(
        &self,
        window: Window,
    ) -> Result<monitor_query::response::Availability, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        type Watermark = (DateTime<Utc>, Option<DateTime<Utc>>, Option<DateTime<Utc>>);
        let (since, evicted, persisted): Watermark = w::table
            .select((
                w::query_watermark_since,
                w::query_watermark_evicted_through,
                w::query_watermark_persisted_at,
            ))
            .first(&mut connection)
            .await?;
        let since = since
            .max(Utc::now() - chrono::Duration::seconds(self.config.retention.0 as i64))
            .max(evicted.unwrap_or(since));
        let gap_count: i64 = crate::schema::history_gap::table
            .filter(crate::schema::history_gap::history_gap_at.ge(window.from))
            .filter(crate::schema::history_gap::history_gap_at.lt(window.to))
            .count()
            .get_result(&mut connection)
            .await?;
        let mut gaps = Vec::new();
        if window.from < since {
            gaps.push("Requested period begins before available diagnostic history".into());
        }
        if gap_count > 0 {
            gaps.push(
                "Persistence dropped records during this period; coverage is incomplete".into(),
            );
        }
        if persisted.is_none() {
            gaps.push("No diagnostic history has been persisted yet".into());
        }
        Ok(monitor_query::response::Availability {
            requested: window,
            available_since: Some(since),
            persisted_through: persisted,
            history_available: true,
            complete: gaps.is_empty(),
            gaps,
        })
    }
}
