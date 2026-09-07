//! Query projections reuse the same allowlisted facts and native context as the dashboard.
use monitor_core::model as core;
use monitor_query::{enums as q, record::*};

macro_rules! convert {
    ($fn:ident, $name:ident, [$($v:ident),+]) => {
        pub fn $fn(value: core::$name) -> q::$name { match value { $(core::$name::$v => q::$name::$v),+ } }
    };
}
convert!(
    provider,
    Provider,
    [Gcp, Aws, Azure, Kubernetes, Github, Edge, Nats]
);
convert!(
    check,
    Check,
    [
        Preflight, Discovery, Inventory, Kubernetes, Edge, Managed, Queues, Releases, Github,
        Metrics, Logs, Alerts, Slo, Flows
    ]
);
convert!(severity, Severity, [Info, Warning, Error]);
convert!(
    expected,
    Expected,
    [Active, Dormant, ScaleToZero, Suspended]
);
convert!(confidence, Confidence, [Direct, Correlated, Insufficient]);
convert!(
    coverage,
    Coverage,
    [
        Complete,
        Denied,
        Unauthenticated,
        Unavailable,
        Unsupported,
        Missing,
        Truncated,
        Timeout,
        Cancelled,
        Malformed,
        Stale,
        InventoryOnly
    ]
);
convert!(
    log_class,
    LogClass,
    [
        Import,
        Panic,
        OutOfMemory,
        Connection,
        Permission,
        Timeout,
        Warning,
        OtherError
    ]
);
convert!(
    health,
    Health,
    [Healthy, Degraded, Unhealthy, Unknown, ExpectedInactive]
);

pub fn scope(target: &crate::view::Target) -> Scope {
    Scope {
        target: target.name.clone(),
        provider: provider(target.provider),
        scope: target.scope.clone(),
    }
}
pub fn location(c: Option<&monitor_core::diagnostics::ResourceContext>) -> Location {
    c.map_or_else(Location::default, |c| Location {
        native_id: Some(c.native_id.clone()),
        region: c.region.clone(),
        zone: c.zone.clone(),
        cluster: c.cluster.clone(),
        namespace: c.namespace.clone(),
        service: Some(c.service.clone()),
        hostname: url::Url::parse(&c.native_id)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned)),
        uid: c.uid.clone(),
        container: c.container.clone(),
    })
}
pub fn facts(data: &core::Data) -> Vec<Fact> {
    crate::facts::facts(data)
        .into_iter()
        .map(|f| Fact {
            label: f.label,
            value: f.value,
        })
        .collect()
}
pub fn links(context: Option<&monitor_core::diagnostics::ResourceContext>) -> Vec<Link> {
    crate::console_links::links(context)
        .into_iter()
        .map(|l| Link {
            label: l.label,
            url: l.url,
        })
        .collect()
}
pub fn finding(f: &crate::view::FindingView, targets: &[crate::view::Target]) -> Option<Record> {
    let target = targets.iter().find(|t| t.name == f.target)?;
    let context = f.diagnostic.as_ref().and_then(|d| d.context.as_ref());
    let mut scope = scope(target);
    if let Some(c) = context {
        scope.provider = provider(c.provider);
        scope.scope = c.scope.clone();
    }
    let at = f
        .diagnostic
        .as_ref()
        .map_or(f.observed_at, |d| d.last_detected_at);
    Some(Record {
        id: f.id.clone(),
        identity: f.id.clone(),
        scope,
        location: location(context),
        check: f.check.map(check),
        resource: Some(f.resource.clone()),
        observed_at: at,
        last_observed_at: at,
        valid_until: f.valid_until,
        closed_at: None,
        stale: f.stale,
        details: Details::Finding {
            rule: f.rule.clone(),
            severity: severity(f.severity),
            state: q::FindingState::Active,
            first_detected_at: f.diagnostic.as_ref().and_then(|d| d.first_detected_at),
            expected: expected(f.expected),
            confidence: confidence(f.confidence),
            facts: f.diagnostic.as_ref().map_or_else(Vec::new, |d| {
                d.facts
                    .iter()
                    .map(|f| Fact {
                        label: f.label.clone(),
                        value: f.value.clone(),
                    })
                    .collect()
            }),
            links: f.diagnostic.as_ref().map_or_else(Vec::new, |d| {
                d.links
                    .iter()
                    .map(|l| Link {
                        label: l.label.clone(),
                        url: l.url.clone(),
                    })
                    .collect()
            }),
            legacy: f.diagnostic.is_none(),
        },
    })
}
pub fn result<'a>(
    result: &'a core::CheckResult,
    job: &'a monitor_core::config::resolve::Job,
    target: &'a crate::view::Target,
    health: &'a std::collections::BTreeMap<String, core::Health>,
) -> impl Iterator<Item = Record> + 'a {
    let id = format!("{}/{:?}", result.target, result.check);
    let record = Record {
        id: id.clone(),
        identity: id,
        scope: scope(target),
        location: Location::default(),
        check: Some(check(result.check)),
        resource: None,
        observed_at: result.finished_at,
        last_observed_at: result.finished_at,
        valid_until: Some(
            result.finished_at + chrono::Duration::seconds(job.settings.freshness() as i64),
        ),
        closed_at: None,
        stale: false,
        details: Details::Check {
            required: job.settings.required,
            started_at: result.started_at,
            finished_at: result.finished_at,
            oldest_observation_at: result.observations.iter().map(|o| o.observed_at).min(),
            complete: result.complete(),
            observations: result.observations.len() as u64,
            interval_seconds: job.settings.interval.0,
            required_failures: result
                .operations
                .iter()
                .filter(|o| o.required && o.coverage != core::Coverage::Complete)
                .count() as u64,
            pending_observations: result
                .observations
                .iter()
                .filter(|o| {
                    o.expected == core::Expected::Active
                        && o.data.is_health_evidence()
                        && health.get(&o.resource) == Some(&core::Health::Unknown)
                })
                .count() as u64,
            operations: result
                .operations
                .iter()
                .filter(|o| o.coverage != core::Coverage::Complete)
                .take(128)
                .map(|o| Operation {
                    name: o.id.clone(),
                    coverage: coverage(o.coverage),
                    required: o.required,
                    observed_at: o.observed_at,
                })
                .collect(),
            operations_truncated: result
                .operations
                .iter()
                .filter(|o| o.coverage != core::Coverage::Complete)
                .count()
                > 128,
        },
    };
    std::iter::once(record).chain(
        result.observations.iter().filter_map(move |obs| {
            crate::query_observations::observation(obs, result, job, target)
        }),
    )
}
