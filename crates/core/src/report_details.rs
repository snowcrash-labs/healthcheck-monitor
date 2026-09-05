//! Compact diagnostic tables expose evidence without raw messages or response bodies.
use crate::{model::*, report::safe};
use std::collections::BTreeMap;
pub fn append(snapshot: &Snapshot, out: &mut String) {
    let mut latest: BTreeMap<&str, &Observation> = BTreeMap::new();
    for observation in snapshot
        .results
        .values()
        .flat_map(|result| &result.observations)
    {
        if latest
            .get(observation.resource.as_str())
            .is_none_or(|previous| previous.observed_at < observation.observed_at)
        {
            latest.insert(&observation.resource, observation);
        }
    }
    let endpoints: Vec<_> = latest
        .values()
        .filter(|observation| matches!(observation.data, Data::Endpoint { .. }))
        .collect();
    if !endpoints.is_empty() {
        out.push_str("\n| Endpoint | Probe purpose | DNS / TLS | HTTP | Latency |\n| --- | --- | --- | --- | --- |\n");
        for observation in endpoints.iter().take(100) {
            if let Data::Endpoint {
                dns,
                tls,
                status,
                latency_ms,
                accepted,
                ..
            } = &observation.data
            {
                out.push_str(&format!(
                    "| {} | {} | {} / {} | {} | {} ms |\n",
                    safe(&observation.resource),
                    if accepted.is_empty() {
                        "Root reachability"
                    } else {
                        "Configured health path"
                    },
                    dns,
                    tls,
                    status.map_or("Unavailable".into(), |status| status.to_string()),
                    latency_ms
                ));
            }
        }
        if endpoints.len() > 100 {
            out.push_str("\nEndpoint table limited to 100 entries; the snapshot contains the remaining observations.\n");
        }
    }
    let mut logs: Vec<_> = latest
        .values()
        .filter_map(|observation| match observation.data {
            Data::Log {
                signature,
                count,
                last_seen,
                sampled,
                ..
            } if signature != LogClass::Warning && count > 0 => Some((
                observation.resource.as_str(),
                signature,
                count,
                last_seen,
                sampled,
            )),
            _ => None,
        })
        .collect();
    logs.sort_by(|left, right| right.2.cmp(&left.2).then_with(|| left.0.cmp(right.0)));
    if !logs.is_empty() {
        out.push_str("\n| Diagnostic scope | Signature | Count | Last seen | Incomplete sample |\n| --- | --- | --- | --- | --- |\n");
        for (resource, signature, count, last_seen, sampled) in logs.iter().take(50) {
            out.push_str(&format!(
                "| {} | {signature:?} | {count} | {} | {sampled} |\n",
                safe(resource),
                last_seen.to_rfc3339()
            ));
        }
        out.push_str(
            "\nCounts describe the collected windows and do not establish complete error rates.\n",
        );
    }
    let windows: Vec<_> = latest
        .values()
        .filter_map(|observation| match observation.data {
            Data::LogWindow {
                start,
                end,
                scanned,
                duplicates,
                limit,
                complete,
                gap_seconds,
            } => Some((
                observation.resource.as_str(),
                start,
                end,
                scanned,
                duplicates,
                limit,
                complete,
                gap_seconds,
            )),
            _ => None,
        })
        .collect();
    if !windows.is_empty() {
        out.push_str("\n| Log window | Start / end | Scanned / limit | Duplicates | Complete | Missing seconds |\n| --- | --- | --- | --- | --- | --- |\n");
        for (resource, start, end, scanned, duplicates, limit, complete, gap) in
            windows.iter().take(50)
        {
            out.push_str(&format!(
                "| {} | {} / {} | {scanned} / {limit} | {duplicates} | {complete} | {gap} |\n",
                safe(resource),
                start.to_rfc3339(),
                end.to_rfc3339()
            ));
        }
    }
}
