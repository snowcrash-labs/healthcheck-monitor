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
    let bus = Bus::new(journal.clone());
    let mut billing = tokio::spawn(crate::cost_worker::run(
        history.clone(),
        config.costs.clone(),
        bus.clone(),
        stop.clone(),
    ));
    let app = Arc::new(App {
        bus: bus.clone(),
        history,
        costs: config.costs.clone(),
        security,
        requests: Arc::new(tokio::sync::Semaphore::new(config.requests)),
        cost_requests: Arc::new(tokio::sync::Semaphore::new(1)),
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
    if tokio::time::timeout_at(deadline, &mut billing)
        .await
        .is_err()
    {
        billing.abort();
        let _ = billing.await;
    }
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
