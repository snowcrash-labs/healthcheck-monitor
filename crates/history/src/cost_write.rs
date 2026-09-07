//! Billing writes use a separate connection and publish only fully staged partitions.
use crate::{
    History,
    cost_rows::{CostProvider, Daily, Source},
    cost_schema::{cost_daily as d, cost_import as i, cost_partition as p, cost_source as s},
    error::Error,
    types::Id,
};
use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, RunQueryDsl};
use monitor_costs::{config, model::Charge, query::Period};

pub struct Import {
    pub id: Id,
    pub source_id: Id,
    pub period: Period,
}
impl History {
    /// Reserve the worst-case scan charge before submitting a paid job; resumes reuse its UUID.
    pub async fn cost_begin(
        &self,
        source: &config::Source,
        period: Period,
        settings: &config::Config,
    ) -> Result<Import, Error> {
        let mut connection = self.cost_pool.get().await.map_err(|_| Error::Pool)?;
        connection
            .transaction(async |c| {
                diesel::insert_into(s::table)
                    .values((
                        s::cost_source_name.eq(&source.id),
                        s::cost_source_provider.eq(CostProvider::from(source.provider)),
                        s::cost_source_scope.eq(&source.billing_scope),
                    ))
                    .on_conflict(s::cost_source_name)
                    .do_nothing()
                    .execute(c)
                    .await?;
                let stored = s::table
                    .filter(s::cost_source_name.eq(&source.id))
                    .select(Source::as_select())
                    .first::<Source>(c)
                    .await?;
                if monitor_costs::model::Provider::from(stored.cost_source_provider)
                    != source.provider
                    || stored.cost_source_scope != source.billing_scope
                {
                    return Err(Error::Record);
                }
                let pending: Option<(Id, NaiveDate, NaiveDate)> = i::table
                    .filter(i::cost_import_source_id.eq(stored.cost_source_id))
                    .filter(i::cost_import_published_at.is_null())
                    .order(i::cost_import_started_at.desc())
                    .select((i::cost_import_id, i::cost_import_from, i::cost_import_to))
                    .first(c)
                    .await
                    .optional()?;
                let (id, from, to) = if let Some(pending) = pending {
                    pending
                } else {
                    let day = Utc::now()
                        .date_naive()
                        .and_hms_opt(0, 0, 0)
                        .ok_or(Error::Record)?
                        .and_utc();
                    let reservations: Vec<i64> = i::table
                        .filter(i::cost_import_started_at.ge(day))
                        .select(i::cost_import_reserved_bytes)
                        .limit(4097)
                        .load(c)
                        .await?;
                    let used = reservations.iter().try_fold(0u64, |sum, n| {
                        sum.checked_add(*n as u64).ok_or(Error::Record)
                    })?;
                    if reservations.len() > 4096
                        || used.saturating_add(settings.query_bytes()) > settings.daily_bytes()
                    {
                        return Err(Error::Record);
                    }
                    let id = diesel::insert_into(i::table)
                        .values((
                            i::cost_import_source_id.eq(stored.cost_source_id),
                            i::cost_import_from.eq(period.from),
                            i::cost_import_to.eq(period.to),
                            i::cost_import_reserved_bytes.eq(settings.query_bytes() as i64),
                        ))
                        .returning(i::cost_import_id)
                        .get_result(c)
                        .await?;
                    (id, period.from, period.to)
                };
                // A restart re-reads immutable job results; partial rows never become visible.
                diesel::delete(d::table.filter(d::cost_daily_import_id.eq(id)))
                    .execute(c)
                    .await?;
                Ok(Import {
                    id,
                    source_id: stored.cost_source_id,
                    period: Period { from, to },
                })
            })
            .await
    }
    pub async fn cost_stage(&self, import: &Import, charges: Vec<Charge>) -> Result<usize, Error> {
        if charges.len() > 500
            || charges
                .iter()
                .any(|r| r.day < import.period.from || r.day >= import.period.to)
        {
            return Err(Error::Record);
        }
        let rows: Vec<_> = charges
            .into_iter()
            .map(|r| Daily::new(import.id, r))
            .collect::<Result<_, _>>()?;
        let mut c = self.cost_pool.get().await.map_err(|_| Error::Pool)?;
        // Duplicated aggregate identities indicate a malformed source, not extra spend.
        diesel::insert_into(d::table)
            .values(rows)
            .execute(&mut c)
            .await
            .map_err(Error::from)
    }
    pub async fn cost_publish(
        &self,
        import: &Import,
        expected_rows: usize,
        retention: u16,
    ) -> Result<(), Error> {
        let mut c = self.cost_pool.get().await.map_err(|_| Error::Pool)?;
        c.transaction(async |c| {
            let count: i64 = d::table
                .filter(d::cost_daily_import_id.eq(import.id))
                .count()
                .get_result(c)
                .await?;
            if count as usize != expected_rows {
                return Err(Error::Record);
            }
            for offset in 0..(import.period.to - import.period.from).num_days() {
                let day = import
                    .period
                    .from
                    .checked_add_signed(chrono::Duration::days(offset))
                    .ok_or(Error::Record)?;
                diesel::insert_into(p::table)
                    .values((
                        p::cost_partition_source_id.eq(import.source_id),
                        p::cost_partition_day.eq(day),
                        p::cost_partition_import_id.eq(import.id),
                    ))
                    .on_conflict((p::cost_partition_source_id, p::cost_partition_day))
                    .do_update()
                    .set(p::cost_partition_import_id.eq(import.id))
                    .execute(c)
                    .await?;
            }
            let now = Utc::now();
            diesel::update(i::table.find(import.id))
                .set(i::cost_import_published_at.eq(now))
                .execute(c)
                .await?;
            diesel::update(s::table.find(import.source_id))
                .set((
                    s::cost_source_revision.eq(Some(*import.id.as_ref())),
                    s::cost_source_imported_at.eq(now),
                    s::cost_source_fault.eq(None::<String>),
                ))
                .execute(c)
                .await?;
            let cutoff = now.date_naive() - chrono::Duration::days(i64::from(retention));
            diesel::delete(p::table.filter(p::cost_partition_day.lt(cutoff)))
                .execute(c)
                .await?;
            Ok(())
        })
        .await
    }
    pub async fn cost_cleanup(&self) -> Result<(), Error> {
        let mut c = self.cost_pool.get().await.map_err(|_| Error::Pool)?;
        let keep = p::table.select(p::cost_partition_import_id);
        let obsolete: Vec<Id> = i::table
            .filter(i::cost_import_published_at.is_not_null())
            .filter(i::cost_import_id.ne_all(keep))
            .filter(
                i::cost_import_id.ne_all(
                    s::table
                        .filter(s::cost_source_revision.is_not_null())
                        .select(s::cost_source_revision.assume_not_null()),
                ),
            )
            .select(i::cost_import_id)
            .limit(16)
            .load(&mut c)
            .await?;
        diesel::delete(i::table.filter(i::cost_import_id.eq_any(obsolete)))
            .execute(&mut c)
            .await?;
        // Superseded days within an import are removed in bounded batches.
        let unused = d::table
            .filter(diesel::dsl::not(diesel::dsl::exists(
                p::table
                    .filter(p::cost_partition_import_id.eq(d::cost_daily_import_id))
                    .filter(p::cost_partition_day.eq(d::cost_daily_day)),
            )))
            .filter(
                d::cost_daily_import_id.eq_any(
                    i::table
                        .filter(i::cost_import_published_at.is_not_null())
                        .select(i::cost_import_id),
                ),
            )
            .select(d::cost_daily_id)
            .limit(5000);
        let unused: Vec<Id> = unused.load(&mut c).await?;
        diesel::delete(d::table.filter(d::cost_daily_id.eq_any(unused)))
            .execute(&mut c)
            .await?;
        Ok(())
    }
    pub async fn cost_fault(&self, source: &str, fault: &str) -> Result<(), Error> {
        if fault.len() > 128 {
            return Err(Error::Record);
        }
        let mut c = self.cost_pool.get().await.map_err(|_| Error::Pool)?;
        diesel::update(s::table.filter(s::cost_source_name.eq(source)))
            .set(s::cost_source_fault.eq(fault))
            .execute(&mut c)
            .await?;
        Ok(())
    }
    pub async fn cost_status(&self) -> Result<Vec<monitor_costs::model::SourceStatus>, Error> {
        let mut c = self.cost_read_pool.get().await.map_err(|_| Error::Pool)?;
        let sources: Vec<Source> = s::table
            .select(Source::as_select())
            .limit(17)
            .load(&mut c)
            .await?;
        if sources.len() > 16 {
            return Err(Error::Record);
        }
        let spans: Vec<(Id, Option<NaiveDate>, Option<NaiveDate>)> = p::table
            .group_by(p::cost_partition_source_id)
            .select((
                p::cost_partition_source_id,
                diesel::dsl::min(p::cost_partition_day),
                diesel::dsl::max(p::cost_partition_day),
            ))
            .load(&mut c)
            .await?;
        Ok(sources
            .into_iter()
            .map(|row| {
                let span = spans.iter().find(|s| s.0 == row.cost_source_id);
                monitor_costs::model::SourceStatus {
                    id: row.cost_source_name,
                    provider: row.cost_source_provider.into(),
                    state: if row.cost_source_fault.is_some() {
                        "failed"
                    } else if row.cost_source_imported_at.is_some() {
                        "provisional"
                    } else {
                        "waiting"
                    }
                    .into(),
                    imported_at: row.cost_source_imported_at,
                    from: span.and_then(|s| s.1),
                    to: span.and_then(|s| s.2).and_then(|d| d.succ_opt()),
                    revision: row.cost_source_revision.map(|id| id.as_ref().to_string()),
                    fault: row.cost_source_fault,
                }
            })
            .collect())
    }
    pub async fn cost_last_import(&self, source: &str) -> Result<Option<DateTime<Utc>>, Error> {
        let mut c = self.cost_read_pool.get().await.map_err(|_| Error::Pool)?;
        s::table
            .filter(s::cost_source_name.eq(source))
            .select(s::cost_source_imported_at)
            .first(&mut c)
            .await
            .optional()
            .map(Option::flatten)
            .map_err(Error::from)
    }
}
