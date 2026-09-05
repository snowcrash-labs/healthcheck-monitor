//! Delayed native-source contracts verify bounded fan-out without cloud credentials.
use monitor_core::{
    config::{resolve::Job, types::Config},
    model::*,
};
use monitor_integrations::{
    kube_collect::{self, Source},
    projection::observation,
    queues,
    transport::Error,
};
use serde_json::{Value, json};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Delayed {
    active: AtomicUsize,
    peak: AtomicUsize,
    calls: AtomicUsize,
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl Source for Delayed {
    async fn json(&self, path: &str, _: &Job, cancel: &CancellationToken) -> Result<Value, Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        let _active = Active(&self.active);
        tokio::select! { _ = cancel.cancelled() => return Err(Error::Cancelled), _ = tokio::time::sleep(Duration::from_millis(20)) => {} }
        if path.contains("external.metrics") {
            return Ok(json!({"items":[{"value":"2500m"}]}));
        }
        if path.contains("/nodes?") {
            return Err(Error::Denied);
        }
        if path.contains("/jobs?") {
            return Err(Error::Authentication);
        }
        if path.contains("/pods?") && !path.contains("continue=two") {
            return Ok(json!({"items":[],"metadata":{"continue":"two"}}));
        }
        Ok(json!({"items":[],"metadata":{}}))
    }
}
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[settings]\nconcurrency=4\nscope_concurrency=4\n[[targets]]\nname='test'\nprovider='kubernetes'\nscope='cluster'")?.resolve(&Default::default())?.jobs.into_iter().find(|job| job.check == Check::Kubernetes).ok_or_else(|| "missing job".into())
}
#[tokio::test(start_paused = true)]
async fn inventory_parallelizes_kinds_preserves_pages_and_independent_failures()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Delayed::default();
    let start = tokio::time::Instant::now();
    let result = kube_collect::collect(&source, &job()?, &CancellationToken::new()).await;
    assert_eq!(source.calls.load(Ordering::SeqCst), 17);
    assert_eq!(source.peak.load(Ordering::SeqCst), 4);
    assert_eq!(source.active.load(Ordering::SeqCst), 0);
    assert!(start.elapsed() < Duration::from_millis(200));
    assert_eq!(result.operations.len(), 16);
    assert!(
        result
            .operations
            .iter()
            .any(|op| op.id == "nodes" && op.coverage == Coverage::Denied)
    );
    assert!(
        result
            .operations
            .iter()
            .any(|op| op.id == "jobs" && op.coverage == Coverage::Unauthenticated)
    );
    assert!(
        result
            .operations
            .iter()
            .any(|op| op.id == "pods" && op.pages == 2 && op.coverage == Coverage::Complete)
    );
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn cancellation_drops_active_reads_and_records_partial_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Delayed::default();
    let job = job()?;
    let cancel = CancellationToken::new();
    let (_, result) = tokio::join!(
        async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            cancel.cancel();
        },
        kube_collect::collect(&source, &job, &cancel)
    );
    assert_eq!(source.active.load(Ordering::SeqCst), 0);
    assert_eq!(source.calls.load(Ordering::SeqCst), 4);
    assert!(
        result
            .operations
            .iter()
            .all(|op| op.coverage == Coverage::Cancelled)
    );
    Ok(())
}
#[tokio::test(start_paused = true)]
async fn queue_demands_share_index_and_preserve_owner_uid_health()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.check = Check::Queues;
    let mut snapshot = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    snapshot.operations.clear();
    for (name, data) in [
        (
            "deployments/ns/worker",
            Data::Workload {
                desired: 2,
                ready: 1,
                created_at: None,
                draining: false,
                node: false,
            },
        ),
        (
            "deployments/ns/worker/owner",
            Data::Owner {
                uid: "deployment-uid".into(),
                owner_uid: None,
            },
        ),
        (
            "replicasets/ns/replica/owner",
            Data::Owner {
                uid: "replica-uid".into(),
                owner_uid: Some("deployment-uid".into()),
            },
        ),
        (
            "pods/ns/pod/owner",
            Data::Owner {
                uid: "pod-uid".into(),
                owner_uid: Some("replica-uid".into()),
            },
        ),
    ] {
        snapshot
            .observations
            .push(observation(&job, "fixture", name, data));
    }
    for index in 0..12 {
        snapshot.observations.push(observation(
            &job,
            "scaledobjects",
            &index.to_string(),
            Data::Scaler {
                namespace: "ns".into(),
                name: format!("scaler-{index}"),
                worker: "worker".into(),
                metric: format!("metric-{index}"),
                activation: 0.0,
                ready: true,
            },
        ));
    }
    let source = Delayed::default();
    let result = queues::collect(&source, &snapshot, &job, &CancellationToken::new()).await;
    assert_eq!(source.peak.load(Ordering::SeqCst), 4);
    assert_eq!(result.observations.len(), 12);
    assert!(result.complete());
    assert!(result.observations.iter().all(|obs| matches!(
        obs.data,
        Data::Queue {
            backlog: 2.5,
            desired: 2,
            ready: 1,
            ..
        }
    )));
    Ok(())
}
