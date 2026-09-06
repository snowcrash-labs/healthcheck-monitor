//! Bounded retention deletes oldest monitor-owned records through typed queries.
use crate::{
    error::Error,
    pool::History,
    schema::{check_run, configuration, finding_event, history_gap},
    types::Id,
};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
impl History {
    pub(crate) async fn ensure_retention(&self) -> Result<(), Error> {
        for _ in 0..8 {
            let mut connection = self.pool.get().await.map_err(|_| Error::Pool)?;
            let counts: (Option<i64>, Option<i64>, Option<i64>, Option<i64>) = diesel::select((
                finding_event::table.count().single_value(),
                check_run::table.count().single_value(),
                history_gap::table.count().single_value(),
                configuration::table.count().single_value(),
            ))
            .get_result(&mut connection)
            .await?;
            drop(connection);
            if counts
                .0
                .is_some_and(|count| count <= self.config.event_rows)
                && counts.1.is_some_and(|count| count <= self.config.run_rows)
                && counts.2.is_some_and(|count| count <= self.config.gap_rows)
                && counts
                    .3
                    .is_some_and(|count| count <= self.config.configuration_rows)
            {
                return Ok(());
            }
            self.retain().await?;
        }
        Err(Error::Capacity)
    }
    /// Each pass bounds both rows deleted and SQL operations; later passes finish any backlog.
    pub async fn retain(&self) -> Result<(), Error> {
        if !self.ready() {
            return Err(Error::Migration);
        }
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(self.config.retention.0 as i64);
        let mut connection = self.pool.get().await.map_err(|_| Error::Pool)?;
        macro_rules! trim {
            ($table:ident, $id:ident, $at:ident, $cap:expr) => {{
                let expired: Vec<Id> = $table::table
                    .filter($table::$at.lt(cutoff))
                    .order($table::$at.asc())
                    .limit(2048)
                    .select($table::$id)
                    .load(&mut connection)
                    .await?;
                if !expired.is_empty() {
                    diesel::delete($table::table.filter($table::$id.eq_any(expired)))
                        .execute(&mut connection)
                        .await?;
                }
                let count: i64 = $table::table.count().get_result(&mut connection).await?;
                let excess = count.saturating_sub($cap).clamp(0, 2048);
                if excess > 0 {
                    let oldest: Vec<Id> = $table::table
                        .order(($table::$at.asc(), $table::$id.asc()))
                        .limit(excess)
                        .select($table::$id)
                        .load(&mut connection)
                        .await?;
                    diesel::delete($table::table.filter($table::$id.eq_any(oldest)))
                        .execute(&mut connection)
                        .await?;
                }
            }};
        }
        trim!(
            finding_event,
            finding_event_id,
            finding_event_at,
            self.config.event_rows
        );
        trim!(
            check_run,
            check_run_id,
            check_run_finished_at,
            self.config.run_rows
        );
        trim!(
            history_gap,
            history_gap_id,
            history_gap_at,
            self.config.gap_rows
        );
        // Configuration eviction also removes its older dependent records through explicit FKs.
        trim!(
            configuration,
            configuration_id,
            configuration_seen_at,
            self.config.configuration_rows
        );
        drop(connection);
        self.retain_queries().await?;
        Ok(())
    }
}
