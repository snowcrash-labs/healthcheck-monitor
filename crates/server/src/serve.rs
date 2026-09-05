//! Supervise HTTP, autonomous collection, and database publication under one bounded shutdown.
use crate::{
    Error,
    api::{App, router},
    bus::Bus,
    config::Config,
    listener::{Listener, Peer},
    security::Security,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
pub async fn serve(
    config_path: &Path,
    options: monitor_runtime::Options,
    duration: Option<Duration>,
    config: Config,
) -> Result<u8, Error> {
    config.validate()?;
    assets(&config.assets)?;
    let security = Security::new(&config, |name| std::env::var(name).ok())?;
    let url = std::env::var(&config.history.database_url_env).map_err(|_| Error::History)?;
    let history =
        monitor_history::History::new(url, config.history.clone()).map_err(|_| Error::History)?;
    if !matches!(
        tokio::time::timeout(Duration::from_secs(5), history.migrate()).await,
        Ok(Ok(()))
    ) {
        tracing::warn!("History initialization deferred; live monitoring remains available");
    }
    let listener = Listener::bind(&config).await?;
    let address = listener.address()?;
    let stop = CancellationToken::new();
    let journal_stop = CancellationToken::new();
    let (journal, mut journal_task) =
        monitor_history::journal::Journal::start(history.clone(), journal_stop.clone());
    let bus = Bus::new(journal.clone(), config.view_bytes);
    let app = Arc::new(App {
        bus: bus.clone(),
        history,
        security,
        requests: Arc::new(tokio::sync::Semaphore::new(config.requests)),
        streams: Arc::new(tokio::sync::Semaphore::new(config.event_streams)),
        stop: stop.clone(),
        response_bytes: config.response_bytes,
        tls: config.tls.is_some(),
    });
    let routes = router(app, &config);
    let monitor_path = config_path.to_owned();
    let monitor_stop = stop.clone();
    let mut monitor = tokio::spawn(async move {
        monitor_runtime::monitor(
            &monitor_path,
            options,
            monitor_core::scheduler::Mode::Watch { duration },
            bus,
            monitor_stop,
        )
        .await
    });
    let http_stop = stop.clone();
    let mut http = tokio::spawn(async move {
        axum::serve(
            listener,
            routes.into_make_service_with_connect_info::<Peer>(),
        )
        .with_graceful_shutdown(http_stop.cancelled_owned())
        .await
    });
    tracing::info!(listen=%address,tls=config.tls.is_some(),http2=true,"Dashboard listening");
    let mut monitor_result = None;
    let mut http_result = None;
    tokio::select! {_=stop.cancelled()=>{},result=&mut monitor=>{monitor_result=Some(result);},result=&mut http=>{http_result=Some(result);}}
    stop.cancel();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    if monitor_result.is_none() {
        match tokio::time::timeout_at(deadline, &mut monitor).await {
            Ok(result) => monitor_result = Some(result),
            Err(_) => {
                monitor.abort();
                let _ = monitor.await;
            }
        }
    }
    if !journal.flush(deadline).await {
        tracing::error!(
            "History flush exceeded shutdown deadline; pending database history may be lost"
        );
    }
    journal_stop.cancel();
    if tokio::time::timeout_at(deadline, &mut journal_task)
        .await
        .is_err()
    {
        journal_task.abort();
        let _ = journal_task.await;
    }
    if http_result.is_none() {
        match tokio::time::timeout_at(deadline, &mut http).await {
            Ok(result) => http_result = Some(result),
            Err(_) => {
                http.abort();
                let _ = http.await;
            }
        }
    }
    if let Some(result) = http_result {
        result.map_err(|_| Error::Task)??;
    }
    match monitor_result {
        Some(result) => Ok(result.map_err(|_| Error::Task)??),
        None => Err(Error::Task),
    }
}
fn assets(root: &Path) -> Result<(), Error> {
    if std::fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(Error::Configuration);
    }
    if !root.join("index.html").is_file() {
        return Err(Error::Configuration);
    }
    let mut pending = vec![root.to_path_buf()];
    let mut count = 0;
    let mut bytes = 0u64;
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            count += 1;
            if kind.is_symlink() || count > 2048 {
                return Err(Error::Configuration);
            }
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                bytes = bytes.saturating_add(entry.metadata()?.len());
                if bytes > 64 * 1024 * 1024 {
                    return Err(Error::Configuration);
                }
            }
        }
    }
    Ok(())
}
