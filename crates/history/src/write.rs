//! Idempotent batched history writes; the database alone allocates primary keys.
use crate::{
    error::Error,
    pool::History,
    records::{Event, Gap, Run},
    rows::{NewConfiguration, NewEvent, NewGap, NewRun},
    schema::{check_run, configuration, finding_event, history_gap},
    types::{Digest, Id},
};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, RunQueryDsl};

impl History {
    pub async fn write(
        &self,
        revision: &Digest,
        runs: &[Run],
        events: &[Event],
        gaps: &[Gap],
    ) -> Result<(), Error> {
        self.write_queries(revision, runs, events, gaps, &[]).await
    }
    pub async fn write_queries(
        &self,
        revision: &Digest,
        runs: &[Run],
        events: &[Event],
        gaps: &[Gap],
        records: &[crate::query_records::QueryRecord],
    ) -> Result<(), Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        if runs.len() > 2048 || events.len() > 10000 || gaps.len() > 32 || records.len() > 20000 {
            return Err(Error::Record);
        }
        self.ensure_retention().await?;
        let mut connection = self.pool.get().await.map_err(|_| Error::Pool)?;
        connection
            .transaction(async |connection| {
                diesel::insert_into(configuration::table)
                    .values(NewConfiguration {
                        configuration_revision: revision.clone(),
                    })
                    .on_conflict(configuration::configuration_revision)
                    .do_update()
                    .set(configuration::configuration_seen_at.eq(diesel::dsl::now))
                    .execute(connection)
                    .await?;
                let id: Id = configuration::table
                    .filter(configuration::configuration_revision.eq(revision))
                    .select(configuration::configuration_id)
                    .first(connection)
                    .await?;
                for batch in runs.chunks(256) {
                    let rows: Vec<_> = batch
                        .iter()
                        .cloned()
                        .map(|row| NewRun::new(id, row))
                        .collect();
                    diesel::insert_into(check_run::table)
                        .values(&rows)
                        .on_conflict(check_run::check_run_key)
                        .do_nothing()
                        .execute(connection)
                        .await?;
                }
                for batch in events.chunks(256) {
                    let rows: Vec<_> = batch
                        .iter()
                        .cloned()
                        .map(|row| NewEvent::new(id, row))
                        .collect();
                    diesel::insert_into(finding_event::table)
                        .values(&rows)
                        .on_conflict(finding_event::finding_event_key)
                        .do_nothing()
                        .execute(connection)
                        .await?;
                }
                for gap in gaps {
                    diesel::insert_into(history_gap::table)
                        .values(NewGap::new(id, gap.clone()))
                        .on_conflict(history_gap::history_gap_key)
                        .do_nothing()
                        .execute(connection)
                        .await?;
                }
                crate::query_write::write(connection, records).await?;
                Ok::<_, Error>(())
            })
            .await?;
        drop(connection);
        self.ensure_retention().await?;
        self.retain_queries().await
    }
}
