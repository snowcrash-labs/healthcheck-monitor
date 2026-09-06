//! One database writer retries bounded batches independently of monitoring and HTTP readers.
use crate::{
    journal::{Batch, Pending, Status},
    pool::History,
    records::Gap,
};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
pub(crate) async fn run(
    history: Arc<History>,
    mut receiver: mpsc::Receiver<Batch>,
    status: Arc<Status>,
    stop: CancellationToken,
) {
    if stop.is_cancelled() {
        return;
    }
    let mut delay = Duration::from_secs(1);
    loop {
        if history.ready() {
            break;
        }
        let initialized =
            tokio::select! { _=stop.cancelled()=>return, result=history.migrate()=>result };
        if initialized.is_ok() {
            break;
        }
        tracing::warn!("History database unavailable; collection continues");
        tokio::select! { _=stop.cancelled()=>return, _=tokio::time::sleep(delay)=>{} }
        delay = (delay * 2).min(Duration::from_secs(60));
    }
    status.available.store(true, Ordering::Release);
    if let Ok(gaps) = history.gaps().await {
        status.gaps.store(gaps.max(0) as u64, Ordering::Relaxed);
    }
    let mut maintenance = tokio::time::interval(Duration::from_secs(30));
    maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_revision: Option<crate::types::Digest> = None;
    loop {
        let pending = status.pending_events.load(Ordering::Acquire) != 0
            || status.pending_runs.load(Ordering::Acquire) != 0;
        let mut batch = if let Some(revision) = last_revision.as_ref().filter(|_| pending) {
            Batch {
                revision: revision.clone(),
                runs: vec![],
                events: vec![],
                records: vec![],
                gaps: vec![],
                _charges: vec![],
                _pending: Pending::new(&status),
            }
        } else {
            tokio::select! {
                _=stop.cancelled()=>break,
                _=maintenance.tick()=>{
                    let result=history.retain().await;
                    if result.is_ok() && let Ok(gaps)=history.gaps().await { status.gaps.store(gaps.max(0) as u64,Ordering::Relaxed); }
                    status.available.store(result.is_ok(),Ordering::Release); continue;
                },
                batch=receiver.recv()=>match batch {Some(batch)=>batch,None=>break},
            }
        };
        last_revision = Some(batch.revision.clone());
        // One packet may already hold a complete check wave; do not clone or buffer raw evidence.
        let events = status.pending_events.swap(0, Ordering::AcqRel);
        let runs = status.pending_runs.swap(0, Ordering::AcqRel);
        if (events != 0 || runs != 0)
            && let Ok(gap) = Gap::new(chrono::Utc::now(), events, runs)
        {
            batch.gaps.push(gap);
        }
        delay = Duration::from_secs(1);
        loop {
            let result = tokio::select! { _=stop.cancelled()=>return, result=history.write_queries(&batch.revision,&batch.runs,&batch.events,&batch.gaps,&batch.records)=>result };
            match result {
                Ok(()) => {
                    if !batch.gaps.is_empty() {
                        status
                            .gaps
                            .fetch_add(batch.gaps.len() as u64, Ordering::Relaxed);
                    }
                    status.available.store(true, Ordering::Release);
                    status
                        .last_write
                        .store(chrono::Utc::now().timestamp_millis(), Ordering::Release);
                    break;
                }
                Err(error) if !error.retryable() => {
                    status.drop_records(
                        (batch.events.len() + batch.records.len()) as u64,
                        batch.runs.len() as u64,
                    );
                    tracing::warn!("Invalid history batch rejected");
                    if batch.events.is_empty() && batch.runs.is_empty() && batch.records.is_empty()
                    {
                        return;
                    }
                    for gap in &batch.gaps {
                        status
                            .pending_events
                            .fetch_add(gap.events.max(0) as u64, Ordering::AcqRel);
                        status
                            .pending_runs
                            .fetch_add(gap.runs.max(0) as u64, Ordering::AcqRel);
                    }
                    break;
                }
                Err(_) => {
                    status.available.store(false, Ordering::Release);
                    tracing::warn!("History write failed; retry scheduled");
                }
            }
            tokio::select! { _=stop.cancelled()=>return, _=tokio::time::sleep(delay)=>{} }
            delay = (delay * 2).min(Duration::from_secs(60));
        }
    }
}
