//! Identical scope semantics for current observations and historical query filters.
use crate::{
    filter::{Filter, Window},
    record::{Location, Record, Scope},
};
pub fn scope(filter: &Filter, scope: &Scope, location: &Location) -> bool {
    filter.target.as_ref().is_none_or(|v| v == &scope.target)
        && filter.provider.is_none_or(|v| v == scope.provider)
        && filter.scope.as_ref().is_none_or(|v| v == &scope.scope)
        && filter
            .region
            .as_ref()
            .is_none_or(|v| Some(v) == location.region.as_ref())
        && filter
            .cluster
            .as_ref()
            .is_none_or(|v| Some(v) == location.cluster.as_ref())
        && filter
            .namespace
            .as_ref()
            .is_none_or(|v| Some(v) == location.namespace.as_ref())
        && filter
            .service
            .as_ref()
            .is_none_or(|v| Some(v) == location.service.as_ref())
        && filter
            .hostname
            .as_ref()
            .is_none_or(|v| Some(v) == location.hostname.as_ref())
}
pub fn record(filter: &Filter, record: &Record) -> bool {
    scope(filter, &record.scope, &record.location)
        && filter.check.is_none_or(|v| Some(v) == record.check)
        && filter
            .resource
            .as_ref()
            .is_none_or(|v| Some(v) == record.resource.as_ref())
        && filter.severity.is_none_or(|v| Some(v) == record.severity())
        && filter.state.is_none_or(|v| Some(v) == record.state())
        && filter.q.as_ref().is_none_or(|q| {
            let q = q.to_lowercase();
            record.identity.to_lowercase().contains(&q)
                || serde_json::to_string(&record.details)
                    .is_ok_and(|s| s.to_lowercase().contains(&q))
        })
}
pub fn overlaps(record: &Record, window: Window) -> bool {
    record.observed_at < window.to
        && (record.last_observed_at >= window.from
            || record.category() == crate::enums::Category::Finding
                && record.closed_at.is_none_or(|at| at >= window.from))
}
