//! Concurrent KEDA demand reads share one owner index and bounded evidence accumulator.
use super::{
    kube_collect::Source,
    projection::{number, observation, operation},
    transport::Error,
};
use futures::{StreamExt, stream};
use monitor_core::{collection_budget::Limit, config::resolve::Job, model::*};
use tokio_util::sync::CancellationToken;
pub async fn collect<S: Source>(
    kube: &S,
    snapshot: &CheckResult,
    job: &Job,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let mut budget = Limit::new(&job.settings);
    budget.operations(
        &mut result.operations,
        snapshot
            .operations
            .iter()
            .filter(|op| {
                matches!(
                    op.id.as_str(),
                    "pods"
                        | "deployments"
                        | "statefulsets"
                        | "replicasets"
                        | "daemonsets"
                        | "scaledobjects"
                        | "horizontalpodautoscalers"
                )
            })
            .cloned(),
    );
    let workers = crate::worker_index::Workers::new(&snapshot.observations);
    let mut reads = stream::iter(
        (0..snapshot.observations.len())
            .filter(|index| matches!(snapshot.observations[*index].data, Data::Scaler { .. })),
    )
    .map(|index| demand(kube, &snapshot.observations[index], &workers, job, cancel))
    .buffer_unordered(crate::admission::width(&job.settings));
    let mut count = 0;
    while let Some((op, observation)) = reads.next().await {
        count += 1;
        let mut op = op;
        if !budget.observations(&mut result.observations, observation)
            && op.coverage == Coverage::Complete
        {
            op.coverage = Coverage::Truncated;
        }
        budget.operations(&mut result.operations, [op]);
    }
    if count == 0 {
        budget.operations(
            &mut result.operations,
            [operation(
                "keda-demand",
                Err(&Error::Unavailable),
                0,
                job.settings.required,
            )],
        );
    }
    budget.finish(&mut result, job.settings.required);
    result.operations.sort_by(|a, b| a.id.cmp(&b.id));
    result
        .observations
        .sort_by(|a, b| a.resource.cmp(&b.resource));
    result.finished_at = chrono::Utc::now();
    result
}
async fn demand<S: Source>(
    kube: &S,
    source: &Observation,
    workers: &crate::worker_index::Workers,
    job: &Job,
    cancel: &CancellationToken,
) -> (Operation, Option<Observation>) {
    let Data::Scaler {
        namespace,
        name,
        worker,
        metric,
        activation,
        ready: scaler_ready,
    } = &source.data
    else {
        return (
            operation(
                "keda-demand",
                Err(&Error::Malformed),
                0,
                job.settings.required,
            ),
            None,
        );
    };
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair(
            "labelSelector",
            &format!("scaledobject.keda.sh/name={name}"),
        )
        .finish();
    let path =
        format!("/apis/external.metrics.k8s.io/v1beta1/namespaces/{namespace}/{metric}?{query}");
    let id = format!("keda/{namespace}/{name}/{metric}");
    let outcome = tokio::select! {
        _ = cancel.cancelled() => Err(Error::Cancelled),
        result = kube.json(&path, job, cancel) => result,
    };
    let value = match outcome {
        Ok(value) => value,
        Err(error) => return (operation(&id, Err(&error), 1, job.settings.required), None),
    };
    let backlog = value
        .pointer("/items")
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|v| {
            number(v, &["/value"]).or_else(|| {
                v.get("value")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.strip_suffix('m'))
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|n| n / 1000.0)
            })
        })
        .filter(|value| value.is_finite() && *value >= 0.0);
    let Some(backlog) = backlog else {
        return (
            operation(&id, Err(&Error::Malformed), 1, job.settings.required),
            None,
        );
    };
    let Some((desired, ready, crash_loop)) = workers.get(namespace, worker) else {
        return (
            operation(&id, Err(&Error::Missing), 1, job.settings.required),
            Some(observation(
                job,
                &id,
                worker,
                Data::Metric {
                    name: metric.clone(),
                    value: backlog,
                    capacity: None,
                    warning: None,
                    error: None,
                    window_seconds: 0,
                },
            )),
        );
    };
    (
        operation(&id, Ok(1), 1, job.settings.required),
        Some(observation(
            job,
            &id,
            worker,
            Data::Queue {
                backlog,
                activation: *activation,
                ready,
                desired,
                crash_loop,
                scaler_ready: *scaler_ready,
                age_seconds: None,
                dead_letters: None,
            },
        )),
    )
}
pub fn worker_state(
    observations: &[Observation],
    namespace: &str,
    worker: &str,
) -> Option<(u32, u32, bool)> {
    crate::worker_index::Workers::new(observations).get(namespace, worker)
}
