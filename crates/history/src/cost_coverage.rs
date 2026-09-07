//! Import coverage counts complete published daily partitions, including zero-charge days.
use crate::{History, cost_schema::{cost_source as s, cost_partition as p}, error::Error};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use monitor_costs::{config::Config, model::SourceStatus, query::{Filter, Period}};

pub fn statuses(config: &Config, filter: &Filter, stored: Vec<SourceStatus>) -> Vec<SourceStatus> {
    config.sources.iter().filter(|source| filter.provider.is_none_or(|p| p == source.provider)).map(|source| {
        let mut status = stored.iter().find(|s| s.id == source.id).cloned().unwrap_or(SourceStatus {
            id: source.id.clone(), provider: source.provider, state: "waiting".into(), imported_at: None, from: None, to: None, revision: None, fault: None,
        });
        if status.fault.is_none() && status.imported_at.is_some_and(|at| chrono::Utc::now().signed_duration_since(at).num_seconds() > config.interval().saturating_mul(2) as i64) { status.state = "stale".into(); }
        status
    }).collect()
}
impl History {
    pub async fn cost_covered(&self, sources: &[SourceStatus], period: Period) -> Result<bool, Error> {
        if sources.is_empty() || sources.iter().any(|s| s.fault.is_some() || s.state == "stale") { return Ok(false); }
        let names: Vec<_> = sources.iter().map(|s| s.id.clone()).collect();
        let mut connection = self.cost_read_pool.get().await.map_err(|_| Error::Pool)?;
        let counts: Vec<(String, i64)> = p::table.inner_join(s::table.on(s::cost_source_id.eq(p::cost_partition_source_id)))
            .filter(s::cost_source_name.eq_any(&names)).filter(p::cost_partition_day.ge(period.from)).filter(p::cost_partition_day.lt(period.to))
            .group_by(s::cost_source_name).select((s::cost_source_name, diesel::dsl::count(p::cost_partition_id))).load(&mut connection).await?;
        let days = (period.to - period.from).num_days();
        Ok(names.iter().all(|name| counts.iter().any(|(n, count)| n == name && *count == days)))
    }
}

