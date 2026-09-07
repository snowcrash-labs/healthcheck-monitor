//! Indexed keyset pagination for diagnostic history and check summaries.
use crate::{
    enums,
    error::Error,
    pool::History,
    rows::{EventRow, RunRow},
    schema::{check_run, finding_event, history_gap},
    types::{Id, Name, Resource},
};
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;

#[derive(Default)]
pub struct Filter {
    pub target: Option<Name>,
    pub resource: Option<Resource>,
    pub severity: Option<enums::Severity>,
    pub kind: Option<enums::Kind>,
    pub before: Option<(DateTime<Utc>, Id)>,
    pub limit: u16,
}
impl History {
    pub async fn events(&self, filter: &Filter) -> Result<Vec<EventRow>, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        if !(1..=100).contains(&filter.limit) {
            return Err(Error::Record);
        }
        let mut query = finding_event::table.into_boxed();
        query = query.filter(
            finding_event::finding_event_at
                .ge(Utc::now() - chrono::Duration::seconds(self.config.retention.0 as i64)),
        );
        if let Some(target) = &filter.target {
            query = query.filter(finding_event::finding_event_target.eq(target));
        }
        if let Some(resource) = &filter.resource {
            query = query.filter(finding_event::finding_event_resource.eq(resource));
        }
        if let Some(severity) = filter.severity {
            query = query.filter(finding_event::finding_event_severity.eq(severity));
        }
        if let Some(kind) = filter.kind {
            query = query.filter(finding_event::finding_event_kind.eq(kind));
        }
        if let Some((at, id)) = filter.before {
            query = query.filter(
                finding_event::finding_event_at
                    .lt(at)
                    .or(finding_event::finding_event_at
                        .eq(at)
                        .and(finding_event::finding_event_id.lt(id))),
            );
        }
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        query
            .order((
                finding_event::finding_event_at.desc(),
                finding_event::finding_event_id.desc(),
            ))
            .limit(i64::from(filter.limit) + 1)
            .select(EventRow::as_select())
            .load(&mut connection)
            .await
            .map_err(Error::from)
    }
    pub async fn runs(
        &self,
        target: Option<&Name>,
        check: Option<enums::Check>,
        before: Option<(DateTime<Utc>, Id)>,
        limit: u16,
    ) -> Result<Vec<RunRow>, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        if !(1..=100).contains(&limit) {
            return Err(Error::Record);
        }
        let mut query = check_run::table.into_boxed();
        query = query.filter(
            check_run::check_run_finished_at
                .ge(Utc::now() - chrono::Duration::seconds(self.config.retention.0 as i64)),
        );
        if let Some(target) = target {
            query = query.filter(check_run::check_run_target.eq(target));
        }
        if let Some(check) = check {
            query = query.filter(check_run::check_run_check.eq(check));
        }
        if let Some((at, id)) = before {
            query = query.filter(
                check_run::check_run_finished_at
                    .lt(at)
                    .or(check_run::check_run_finished_at
                        .eq(at)
                        .and(check_run::check_run_id.lt(id))),
            );
        }
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        query
            .order((
                check_run::check_run_finished_at.desc(),
                check_run::check_run_id.desc(),
            ))
            .limit(i64::from(limit) + 1)
            .select(RunRow::as_select())
            .load(&mut connection)
            .await
            .map_err(Error::from)
    }
    pub async fn gaps(&self) -> Result<i64, Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        let mut connection = self.read_pool.get().await.map_err(|_| Error::Pool)?;
        history_gap::table
            .count()
            .get_result(&mut connection)
            .await
            .map_err(Error::from)
    }
}
