//! Foreground supervision, atomic configuration reload, and partial evidence.
use crate::args::Options;
use monitor_core::{
    config::types::Config,
    error::Error,
    model::Transition,
    report::exit_code,
    scheduler::{Mode, drive},
    state::State,
    storage::Store,
};
use monitor_providers::router::Router;
use std::{path::Path, sync::Arc};
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::{mpsc, watch},
};
use tokio_util::sync::CancellationToken;
pub fn load(path: &Path) -> Result<Config, Error> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut text)?;
    Config::parse(&text)
}
pub async fn monitor(config_path: &Path, options: Options, mode: Mode) -> Result<u8, Error> {
    let config = load(config_path)?;
    let selection = options.selection.selection();
    let mut effective = config.resolve(&selection)?;
    let mut settings = effective
        .jobs
        .first()
        .ok_or(Error::Evidence)?
        .settings
        .clone();
    let store = Arc::new(Store::open(&options.output)?);
    let mut state = match store.latest(settings.memory_bytes / 2)? {
        Some(snapshot) => State { snapshot },
        None => State::new(
            effective.revision.clone(),
            effective.jobs.iter().map(|j| j.key.clone()).collect(),
        ),
    };
    state.retain_scope(&effective.jobs);
    let router = Arc::new(Router::new(config, settings.subprocesses));
    let stop = CancellationToken::new();
    let (updates, receiver) = watch::channel(effective.clone());
    let (sender, mut results) = mpsc::channel(settings.concurrency);
    let task = tokio::spawn(drive(router.clone(), receiver, mode, sender, stop.clone()));
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let mut reload = signal(SignalKind::hangup())?;
    let mut history = tokio::time::interval(settings.history_interval.duration());
    history.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    history.tick().await;
    let mut transitions: Vec<Transition> = Vec::new();
    let mut cancelled = false;
    loop {
        tokio::select! {
            _ = interrupt.recv(), if !cancelled => { cancelled = true; stop.cancel(); },
            _ = terminate.recv(), if !cancelled => { cancelled = true; stop.cancel(); },
            _ = reload.recv(), if matches!(mode, Mode::Watch { .. }) && !cancelled => {
                match load(config_path).and_then(|config| config.resolve(&selection).map(|e| (config, e))) {
                    Ok((config, replacement)) => {
                        router.reload(config).await;
                        state.retain_scope(&replacement.jobs);
                        if let Some(job) = replacement.jobs.first() { settings = job.settings.clone(); }
                        history = tokio::time::interval(settings.history_interval.duration());
                        history.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                        effective = replacement.clone();
                        if updates.send(replacement).is_err() { stop.cancel(); }
                        tracing::info!(revision = %effective.revision, "Configuration reloaded");
                    }
                    Err(_) => tracing::warn!("Configuration reload rejected; previous configuration retained"),
                }
            },
            _ = history.tick(), if matches!(mode, Mode::Watch { .. }) => {
                transitions.extend(state.expire(settings.freshness(), chrono::Utc::now()));
                publish(&store, &mut state, &mut transitions, &settings).await;
            },
            result = results.recv() => {
                let Some((job, result)) = result else { break };
                if !effective.jobs.iter().any(|j| j.key == job.key && j.revision == job.revision) { continue; }
                tracing::info!(target_name = %job.target.name, check = ?job.check, complete = result.complete(), observations = result.observations.len(), "Check finished");
                let new = state.apply(&job, result, chrono::Utc::now());
                let remaining = settings.max_findings.saturating_mul(2).saturating_sub(transitions.len());
                if new.len() > remaining { state.snapshot.persistence_fault = true; }
                transitions.extend(new.into_iter().take(remaining));
            }
        }
    }
    task.await.map_err(|_| Error::Evidence)?;
    publish(&store, &mut state, &mut transitions, &settings).await;
    if cancelled {
        return Ok(130);
    }
    Ok(match mode {
        Mode::Once => exit_code(&state.snapshot, options.strict),
        Mode::Watch { .. } => 0,
    })
}
async fn publish(
    store: &Arc<Store>,
    state: &mut State,
    transitions: &mut Vec<Transition>,
    settings: &monitor_core::config::settings::Settings,
) {
    let store = store.clone();
    let mut snapshot = state.snapshot.clone();
    snapshot.captured_at = chrono::Utc::now();
    snapshot.persistence_fault = false;
    let events = transitions.clone();
    let settings = settings.clone();
    match tokio::task::spawn_blocking(move || store.publish(&snapshot, &events, &settings)).await {
        Ok(Ok(())) => {
            state.snapshot.persistence_fault = false;
            transitions.clear();
        }
        _ => {
            state.snapshot.persistence_fault = true;
            tracing::error!("Persistence failed; collection continues within configured bounds");
        }
    }
}
