//! Compact log and release evidence preserves native observation windows.
use crate::query_projection::{check, facts, links, location, provider, scope};
use chrono::{DateTime, Utc};
use monitor_core::model as core;
use monitor_query::record::*;
pub fn observation(
    obs: &core::Observation,
    result: &core::CheckResult,
    job: &monitor_core::config::resolve::Job,
    target: &crate::view::Target,
) -> Option<Record> {
    let mut scope = scope(target);
    if let Some(c) = &obs.context {
        scope.provider = provider(c.provider);
        scope.scope = c.scope.clone();
    }
    let details = match &obs.data {
        core::Data::Log {
            signature,
            count,
            first_seen,
            last_seen,
            sampled,
        } => {
            let window = result.observations.iter().find(|o| {
                o.operation == obs.operation && matches!(o.data, core::Data::LogWindow { .. })
            });
            let (start, end, scanned, duplicates, complete, gap_seconds) =
                match window.map(|o| &o.data) {
                    Some(core::Data::LogWindow {
                        start,
                        end,
                        scanned,
                        duplicates,
                        complete,
                        gap_seconds,
                        ..
                    }) => (
                        *start,
                        *end,
                        *scanned as u64,
                        *duplicates as u64,
                        *complete,
                        *gap_seconds,
                    ),
                    _ => (*first_seen, *last_seen, 0, 0, false, 0),
                };
            let mut links = links(obs.context.as_ref());
            links.extend(
                crate::log_links::links(obs.context.as_ref(), &obs.data)
                    .into_iter()
                    .map(|l| Link {
                        label: l.label,
                        url: l.url,
                    }),
            );
            Details::Diagnostic {
                signature: format!("{signature:?}"),
                count: *count,
                first_seen: *first_seen,
                last_seen: *last_seen,
                window_start: start,
                window_end: end,
                sampled: *sampled,
                scanned,
                duplicates,
                complete,
                gap_seconds,
                links,
            }
        }
        core::Data::Image {
            observed_digest,
            revision,
            ..
        } => Details::Release {
            revision: revision.clone(),
            observed_digests: observed_digest.iter().cloned().collect(),
            desired_digest: None,
            pending: observed_digest.is_none(),
            verified: false,
            facts: facts(&obs.data),
            links: links(obs.context.as_ref()),
        },
        core::Data::Provenance {
            observed_digests,
            desired_digest,
            pending,
            revision,
            registry_verified,
            build_verified,
            commit_verified,
            mismatch,
            ..
        } => Details::Release {
            revision: revision.clone(),
            observed_digests: observed_digests.clone(),
            desired_digest: desired_digest.clone(),
            pending: *pending,
            verified: *registry_verified && *build_verified && *commit_verified && !*mismatch,
            facts: facts(&obs.data),
            links: links(obs.context.as_ref()),
        },
        _ => return None,
    };
    let at: DateTime<Utc> = match &details {
        Details::Diagnostic { first_seen, .. } => *first_seen,
        _ => obs.observed_at,
    };
    let last = match &details {
        Details::Diagnostic { last_seen, .. } => *last_seen,
        _ => obs.observed_at,
    };
    Some(Record {
        id: obs.resource.clone(),
        identity: format!("{}/{:?}/{}", obs.resource, result.check, obs.operation),
        scope,
        location: location(obs.context.as_ref()),
        check: Some(check(result.check)),
        resource: Some(obs.resource.clone()),
        observed_at: at,
        last_observed_at: last,
        valid_until: Some(
            obs.observed_at + chrono::Duration::seconds(job.settings.freshness() as i64),
        ),
        closed_at: None,
        stale: false,
        details,
    })
}
