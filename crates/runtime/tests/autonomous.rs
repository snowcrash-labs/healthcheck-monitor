//! Foreground monitoring continues without HTTP clients or browser-triggered work.
use monitor_core::{config::resolve::Selection, model::Check, scheduler::Mode};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
struct Observer(AtomicUsize);
impl monitor_runtime::Observer for Observer {
    fn update(
        &self,
        snapshot: &monitor_core::model::Snapshot,
        _: &monitor_core::config::resolve::Effective,
        _: &[monitor_core::model::Transition],
    ) {
        if !snapshot.results.is_empty() {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn heartbeat(&self, _: chrono::DateTime<chrono::Utc>, _: bool) {}
}
#[tokio::test]
async fn duration_watch_publishes_without_any_readers() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("monitor.toml");
    std::fs::write(
        &path,
        "version=1\n[[targets]]\nname='fixture'\nprovider='edge'\nscope='fixture'",
    )?;
    let observer = Arc::new(Observer(AtomicUsize::new(0)));
    let output = directory.path().join("evidence");
    let status = monitor_runtime::monitor(
        &path,
        monitor_runtime::Options {
            selection: Selection {
                checks: vec![Check::Preflight],
                ..Default::default()
            },
            output: output.clone(),
            strict: false,
        },
        Mode::Watch {
            duration: Some(Duration::from_millis(100)),
        },
        observer.clone(),
        tokio_util::sync::CancellationToken::new(),
    )
    .await?;
    assert_eq!(status, 0);
    assert!(observer.0.load(Ordering::SeqCst) > 0);
    assert!(output.join("monitor-latest.json").is_file());
    Ok(())
}
