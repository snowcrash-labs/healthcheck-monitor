//! Weekly billing partitions bound backfill work and keep recent costs refreshed first.
use crate::{
    History,
    cost_schema::{cost_import as i, cost_partition as p, cost_source as s},
    error::Error,
};
use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use monitor_costs::{
    config::{Config, Source},
    query::Period,
};
impl History {
    pub async fn cost_next_period(
        &self,
        source: &Source,
        config: &Config,
    ) -> Result<Option<Period>, Error> {
        let now = Utc::now();
        let to = now.date_naive().succ_opt().ok_or(Error::Record)?;
        let from = to - chrono::Duration::days(i64::from(config.backfill()));
        let mut c = self.cost_read_pool.get().await.map_err(|_| Error::Pool)?;
        let rows: Vec<(NaiveDate, Option<DateTime<Utc>>)> = p::table
            .inner_join(s::table.on(s::cost_source_id.eq(p::cost_partition_source_id)))
            .inner_join(i::table.on(i::cost_import_id.eq(p::cost_partition_import_id)))
            .filter(s::cost_source_name.eq(&source.id))
            .filter(
                p::cost_partition_day
                    .ge(to - chrono::Duration::days(i64::from(config.retention()))),
            )
            .select((p::cost_partition_day, i::cost_import_published_at))
            .limit(731)
            .load(&mut c)
            .await?;
        if rows.len() > 730 {
            return Err(Error::Record);
        }
        let cadence = if source.aws_query.is_some() {
            86400
        } else {
            config.interval() as i64
        };
        let recent = rows
            .iter()
            .find(|(day, _)| *day == now.date_naive())
            .and_then(|(_, at)| *at);
        let window = |end: NaiveDate| Period {
            // Cost Explorer charges per request; its bounded aggregate query covers the configured history.
            from: if source.aws_query.is_some() {
                from
            } else {
                (end - chrono::Duration::days(7)).max(from)
            },
            to: end,
        };
        if recent.is_none_or(|at| now.signed_duration_since(at).num_seconds() >= cadence) {
            return Ok(Some(window(to)));
        }
        for offset in 0..i64::from(config.backfill()) {
            let day = to - chrono::Duration::days(offset + 1);
            if !rows.iter().any(|(seen, _)| *seen == day) {
                return Ok(day.succ_opt().map(window));
            }
        }
        // Revisit closed retained partitions for late adjustments without replaying all history hourly.
        if let Some((day, _)) = rows
            .iter()
            .filter(|(_, at)| at.is_some_and(|at| now.signed_duration_since(at).num_days() >= 30))
            .min_by_key(|(_, at)| *at)
        {
            return Ok(Some(Period {
                from: *day,
                to: day.succ_opt().ok_or(Error::Record)?,
            }));
        }
        Ok(None)
    }
}
