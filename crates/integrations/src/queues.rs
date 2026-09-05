//! KEDA demand correlated with workload readiness and authoritative owner links.
use super::{
    kubernetes::Kubernetes,
    projection::{number, observation, operation},
    transport::Error,
};
use monitor_core::{config::resolve::Job, model::*};
use std::collections::BTreeSet;
use tokio_util::sync::CancellationToken;
pub async fn collect(
    kube: &Kubernetes,
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
    result.operations = snapshot
        .operations
        .iter()
        .filter(|operation| {
            matches!(
                operation.id.as_str(),
                "pods"
                    | "deployments"
                    | "statefulsets"
                    | "replicasets"
                    | "scaledobjects"
                    | "horizontalpodautoscalers"
            )
        })
        .cloned()
        .collect();
    let mut count = 0;
    for source in &snapshot.observations {
        let Data::Scaler {
            namespace,
            name,
            worker,
            metric,
            activation,
            ready: scaler_ready,
        } = &source.data
        else {
            continue;
        };
        count += 1;
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair(
                "labelSelector",
                &format!("scaledobject.keda.sh/name={name}"),
            )
            .finish();
        let path = format!(
            "/apis/external.metrics.k8s.io/v1beta1/namespaces/{namespace}/{metric}?{query}"
        );
        let id = format!("keda/{namespace}/{name}/{metric}");
        match kube.json(&path, job, cancel).await {
            Ok(value) => {
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
                    });
                let Some(backlog) = backlog else {
                    result
                        .operations
                        .push(operation(&id, Err(&Error::Malformed), 1, true));
                    continue;
                };
                let Some((desired, ready, crash_loop)) =
                    worker_state(&snapshot.observations, namespace, worker)
                else {
                    result.observations.push(observation(
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
                    ));
                    result
                        .operations
                        .push(operation(&id, Err(&Error::Missing), 1, true));
                    continue;
                };
                result.observations.push(observation(
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
                ));
                result.operations.push(operation(&id, Ok(1), 1, true));
            }
            Err(error) => result.operations.push(operation(&id, Err(&error), 1, true)),
        }
    }
    if count == 0 {
        result
            .operations
            .push(operation("keda-demand", Err(&Error::Unavailable), 0, true));
    }
    result.finished_at = chrono::Utc::now();
    result
}
pub fn worker_state(
    observations: &[Observation],
    namespace: &str,
    worker: &str,
) -> Option<(u32, u32, bool)> {
    let suffix = format!("/{namespace}/{worker}");
    let resource = observations.iter().find(|o| {
        o.resource.ends_with(&suffix) && matches!(o.data, Data::Workload { node: false, .. })
    });
    let workload = resource?;
    let (desired, ready) = match workload.data {
        Data::Workload { desired, ready, .. } => (desired, ready),
        _ => (0, 0),
    };
    let owner_key = format!("{}/owner", workload.resource);
    let mut owners: BTreeSet<String> = observations
        .iter()
        .filter(|o| o.resource == owner_key)
        .filter_map(|o| match &o.data {
            Data::Owner { uid, .. } => Some(uid.clone()),
            _ => None,
        })
        .collect();
    // Deployment -> ReplicaSet -> Pod; deeper graphs are not inferred from name prefixes.
    for _ in 0..3 {
        let children: Vec<_> = observations
            .iter()
            .filter_map(|o| match &o.data {
                Data::Owner {
                    uid,
                    owner_uid: Some(parent),
                } if owners.contains(parent) => Some(uid.clone()),
                _ => None,
            })
            .collect();
        owners.extend(children);
    }
    let crash = observations
        .iter()
        .any(|o| matches!(&o.data,Data::Pod{uid,crash_loop:true,..}if owners.contains(uid)));
    Some((desired, ready, crash))
}
