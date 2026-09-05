//! Foreground supervision, atomic configuration reload, and partial evidence.
use crate::args::Options;
use monitor_core::{
    config::types::Config,
    error::Error,
    model::Transition,
    publication::Publisher,
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
    let mut config = Config::parse(&text)?;
    if let Some(base) = path.parent() {
        for credential in config.credentials.values_mut() {
            if let Some(file) = credential
                .credential_file
                .as_mut()
                .filter(|file| file.is_relative())
            {
                *file = base.join(&*file);
            }
        }
    }
    Ok(config)
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
    if matches!(mode, Mode::Once) {
        state.begin_run();
    }
    let router = Arc::new(Router::new(config, &effective));
    if matches!(mode, Mode::Watch { .. }) {
        router.restore_logs(&state.snapshot).await;
    }
    let stop = CancellationToken::new();
    let expiry = match mode {
        Mode::Watch {
            duration: Some(duration),
        } => Some(tokio::time::Instant::now() + duration),
        _ => None,
    };
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
    let mut publisher = Publisher::new(store);
    let mut disk_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    disk_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut cancelled = false;
    let mut cleanup_deadline = None;
    loop {
        tokio::select! {
            _ = interrupt.recv(), if cleanup_deadline.is_none() => { cancelled = true; stop.cancel(); cleanup_deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(4)); },
            _ = terminate.recv(), if cleanup_deadline.is_none() => { cancelled = true; stop.cancel(); cleanup_deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(4)); },
            _ = until(expiry), if cleanup_deadline.is_none() => { stop.cancel(); cleanup_deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(4)); },
            _ = until(cleanup_deadline) => { task.abort(); break; },
            _ = reload.recv(), if matches!(mode, Mode::Watch { .. }) && cleanup_deadline.is_none() => {
                match load(config_path).and_then(|config| config.resolve(&selection).map(|e| (config, e))) {
                    Ok((config, replacement)) => {
                        router.reload(config, &replacement).await;
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
            _ = history.tick(), if matches!(mode, Mode::Watch { .. }) && cleanup_deadline.is_none() => {
                let expired = state.expire(settings.freshness(), chrono::Utc::now());
                append(&mut transitions, expired, &settings, &mut state);
                publisher.request();
                publisher.start(&state.snapshot, &transitions, &settings);
            },
            _ = disk_tick.tick(), if matches!(mode, Mode::Watch { .. }) && cleanup_deadline.is_none() => {
                match publisher.poll().await {
                    Some(Ok(count)) => { transitions.drain(..count.min(transitions.len())); state.snapshot.persistence_fault = false; },
                    Some(Err(())) => { state.snapshot.persistence_fault = true; tracing::error!("Persistence failed; retry scheduled while collection continues"); },
                    None => {},
                }
                publisher.start(&state.snapshot, &transitions, &settings);
            },
            result = results.recv() => {
                let Some((job, result)) = result else { break };
                if !effective.jobs.iter().any(|j| j.key == job.key && j.revision == job.revision) { continue; }
                tracing::info!(target_name = %job.target.name, check = ?job.check, complete = result.complete(), observations = result.observations.len(), "Check finished");
                let mut new = state.apply(&job, result, chrono::Utc::now());
                if matches!(job.check,monitor_core::model::Check::Inventory|monitor_core::model::Check::Kubernetes|monitor_core::model::Check::Github|monitor_core::model::Check::Releases) {new.extend(state.refresh_releases(&effective.jobs,chrono::Utc::now()));}
                append(&mut transitions, new, &settings, &mut state);
            }
        }
    }
    if let Err(error) = task.await
        && !error.is_cancelled()
    {
        return Err(Error::Evidence);
    }
    state.snapshot.persistence_fault = !publisher
        .finish(
            &state.snapshot,
            &mut transitions,
            &settings,
            cleanup_deadline
                .unwrap_or_else(|| tokio::time::Instant::now() + std::time::Duration::from_secs(4)),
        )
        .await;
    if state.snapshot.persistence_fault {
        tracing::error!("Final evidence publication failed or exceeded cleanup deadline");
    }
    if cancelled {
        return Ok(130);
    }
    Ok(match mode {
        Mode::Once => exit_code(&state.snapshot, options.strict),
        Mode::Watch { .. } => 0,
    })
}
async fn until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}
fn append(
    transitions: &mut Vec<Transition>,
    incoming: Vec<Transition>,
    settings: &monitor_core::config::settings::Settings,
    state: &mut State,
) {
    let remaining = settings
        .max_findings
        .saturating_mul(2)
        .saturating_sub(transitions.len());
    if incoming.len() > remaining {
        state.snapshot.dropped_transitions = state
            .snapshot
            .dropped_transitions
            .saturating_add((incoming.len() - remaining) as u64);
        state.snapshot.persistence_fault = true;
        tracing::error!(
            discarded = incoming.len() - remaining,
            "Transition retention limit reached"
        );
    }
    transitions.extend(incoming.into_iter().take(remaining));
}
