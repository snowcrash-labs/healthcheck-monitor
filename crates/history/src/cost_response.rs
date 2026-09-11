//! Complete one billing response after verifying its publication did not change.
use crate::{History, cost_cursor::encode_cursor, error::Error};
use monitor_costs::{
    aggregate::{Bucket, Contributor, View},
    config::Config,
    model::SourceStatus,
    query::{Filter, Period},
};
pub(crate) struct Draft {
    pub buckets: Vec<Bucket>,
    pub breakdown: Vec<Contributor>,
    pub contributor_count: i64,
    pub period: Period,
    pub previous: Period,
    pub revision: String,
    pub fingerprint: String,
    pub limit: usize,
}
impl History {
    pub(crate) async fn cost_response(
        &self,
        filter: &Filter,
        config: &Config,
        sources: Vec<SourceStatus>,
        data: Draft,
    ) -> Result<View, Error> {
        let Draft {
            buckets,
            mut breakdown,
            contributor_count,
            period,
            previous,
            revision,
            fingerprint,
            limit,
        } = data;
        let has_data = buckets
            .iter()
            .any(|b| b.day >= period.from && b.day < period.to);
        let previous_complete = period.to <= chrono::Utc::now().date_naive()
            && self.cost_covered(&sources, previous).await?
            && self.cost_covered(&sources, period).await?;
        let total = if has_data {
            Some(monitor_costs::aggregate::total(&buckets, period).map_err(|_| Error::Record)?)
        } else {
            None
        };
        let credits = if has_data {
            Some(monitor_costs::aggregate::credits(&buckets, period).map_err(|_| Error::Record)?)
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
            breakdown
                .get(limit - 1)
                .map(|row| encode_cursor(row, &revision, &fingerprint))
                .transpose()?
        } else {
            None
        };
        breakdown.truncate(limit);
        if !previous_complete {
            for row in &mut breakdown {
                row.previous = None;
            }
        }
        // Do not mix new partitions into a response selected against an older publication.
        let after = crate::cost_coverage::statuses(config, filter, self.cost_status().await?);
        if serde_json::to_vec(&sources).ok() != serde_json::to_vec(&after).ok() {
            return Err(Error::Revision);
        }
        Ok(View {
            enabled: true,
            revision,
            period,
            currency: filter.currency.clone().unwrap_or_else(|| "USD".into()),
            measure: filter.measure,
            group: filter.group,
            granularity: filter.granularity,
            total,
            credits,
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
