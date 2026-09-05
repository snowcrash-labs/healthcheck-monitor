//! Journal admission and shutdown accounting remain bounded even without a database writer.
use crate::{
    journal::{Journal, Status},
    records::Run,
    types::Digest,
};
use monitor_core::{
    budget::Budget,
    model::{Check, CheckResult, Coverage},
};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

fn run() -> Result<Run, crate::error::Error> {
    Run::new(&CheckResult::failure(
        "fixture".into(),
        Check::Edge,
        "a".repeat(64),
        Coverage::Denied,
    ))
}

#[tokio::test]
async fn dequeue_does_not_allow_flush_before_the_write_finishes()
-> Result<(), Box<dyn std::error::Error>> {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let journal = Journal {
        sender,
        bytes: Arc::new(Budget::new(65536)),
        status: Arc::new(Status::default()),
    };
    journal.submit(Digest::try_new("a".repeat(64))?, [run()], []);
    let batch = receiver.try_recv()?;
    assert_eq!(journal.health().queued_batches, 0);
    assert_eq!(journal.status.outstanding.load(Ordering::Acquire), 1);
    assert!(
        !journal
            .flush(tokio::time::Instant::now() + Duration::from_millis(10))
            .await
    );
    drop(batch);
    assert!(
        journal
            .flush(tokio::time::Instant::now() + Duration::from_millis(10))
            .await
    );
    Ok(())
}

#[tokio::test]
async fn failed_admission_releases_charges_and_records_every_missing_run()
-> Result<(), Box<dyn std::error::Error>> {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let journal = Journal {
        sender,
        bytes: Arc::new(Budget::new(65536)),
        status: Arc::new(Status::default()),
    };
    for _ in 0..1000 {
        journal.submit(Digest::try_new("a".repeat(64))?, [run()], []);
    }
    assert_eq!(journal.health().queued_batches, 1);
    assert_eq!(journal.status.outstanding.load(Ordering::Acquire), 1);
    assert_eq!(journal.health().dropped_runs, 999);
    assert!(!journal.health().available);
    drop(receiver.try_recv()?);
    assert_eq!(journal.status.outstanding.load(Ordering::Acquire), 0);
    assert_eq!(journal.status.pending_runs.load(Ordering::Acquire), 999);
    Ok(())
}
