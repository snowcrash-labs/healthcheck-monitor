//! Disk faults and slow publication must not create overlapping or unbounded writers.
use super::*;
use crate::{model::TransitionKind, state::State};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Fixture {
    failures: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
    calls: AtomicUsize,
}
impl Fixture {
    fn new(failures: usize) -> Self {
        Self {
            failures: AtomicUsize::new(failures),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        }
    }
}
impl Sink for Fixture {
    fn publish(&self, _: &Snapshot, _: &[Transition], _: &Settings) -> Result<(), Error> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(10));
        self.active.fetch_sub(1, Ordering::SeqCst);
        if self
            .failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_sub(1)
            })
            .is_ok()
        {
            return Err(Error::Io(std::io::Error::from_raw_os_error(28)));
        }
        Ok(())
    }
}
fn event() -> Transition {
    Transition {
        at: chrono::Utc::now(),
        finding: "resource/rule".into(),
        kind: TransitionKind::New,
    }
}
#[tokio::test(start_paused = true)]
async fn disk_exhaustion_retries_with_bounded_backoff_and_preserves_events()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = Arc::new(Fixture::new(3));
    let mut publisher = Publisher::new(sink.clone());
    let state = State::new("test".into(), vec![]);
    let settings = Settings::default();
    let events = vec![event()];
    publisher.request();
    for backoff in [1, 2, 4] {
        assert!(publisher.start(&state.snapshot, &events, &settings));
        assert_eq!(publisher.complete().await, Some(Err(())));
        assert!(!publisher.start(&state.snapshot, &events, &settings));
        assert!(publisher.next <= Instant::now() + Duration::from_secs(backoff));
        tokio::time::advance(Duration::from_secs(backoff)).await;
    }
    assert!(publisher.start(&state.snapshot, &events, &settings));
    assert_eq!(publisher.complete().await, Some(Ok(1)));
    assert_eq!(sink.peak.load(Ordering::SeqCst), 1);
    assert_eq!(sink.calls.load(Ordering::SeqCst), 4);
    assert_eq!(publisher.backoff, Duration::from_secs(1));
    Ok(())
}
#[tokio::test]
async fn slow_writer_coalesces_requests_and_acknowledges_only_its_event_prefix()
-> Result<(), Box<dyn std::error::Error>> {
    let sink = Arc::new(Fixture::new(0));
    let mut publisher = Publisher::new(sink.clone());
    let state = State::new("test".into(), vec![]);
    let settings = Settings::default();
    let mut events = vec![event()];
    publisher.request();
    assert!(publisher.start(&state.snapshot, &events, &settings));
    events.push(event());
    for _ in 0..1000 {
        publisher.request();
        assert!(!publisher.start(&state.snapshot, &events, &settings));
    }
    assert_eq!(publisher.complete().await, Some(Ok(1)));
    events.drain(..1);
    assert!(publisher.start(&state.snapshot, &events, &settings));
    assert_eq!(publisher.complete().await, Some(Ok(1)));
    assert_eq!(sink.peak.load(Ordering::SeqCst), 1);
    assert_eq!(sink.calls.load(Ordering::SeqCst), 2);
    Ok(())
}
#[tokio::test]
async fn elapsed_shutdown_deadline_does_not_start_another_disk_write() {
    let sink = Arc::new(Fixture::new(0));
    let mut publisher = Publisher::new(sink.clone());
    let state = State::new("test".into(), vec![]);
    assert!(
        !publisher
            .finish(
                &state.snapshot,
                &mut vec![],
                &Settings::default(),
                Instant::now()
            )
            .await
    );
    assert_eq!(sink.calls.load(Ordering::SeqCst), 0);
}
