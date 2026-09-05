//! Synthetic read views exercise the server without contacting monitored infrastructure.
use crate::{api::App, bus::Bus, config::Config, security::Security};
use monitor_core::{
    config::{resolve::Effective, types::Config as MonitorConfig},
    model::*,
    state::State,
};
use monitor_runtime::Observer;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
pub async fn app(config: &Config) -> Result<(Arc<App>, Effective), Box<dyn std::error::Error>> {
    let history = monitor_history::History::new(
        "postgresql:///healthcheck_monitor_dashboard_test?host=/tmp".into(),
        Default::default(),
    )?;
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let (journal, task) = monitor_history::journal::Journal::start(history.clone(), cancelled);
    task.await?;
    let bus = Bus::new(journal, config.view_bytes);
    let (state, effective) = evidence()?;
    bus.update(&state.snapshot, &effective, &[]);
    bus.heartbeat(chrono::Utc::now(), true);
    let app = Arc::new(App {
        bus,
        history,
        security: Security::new(config, |_| Some("s".repeat(32)))?,
        requests: Arc::new(tokio::sync::Semaphore::new(config.requests)),
        streams: Arc::new(tokio::sync::Semaphore::new(config.event_streams)),
        stop: CancellationToken::new(),
        response_bytes: config.response_bytes,
        tls: config.tls.is_some(),
    });
    Ok((app, effective))
}
pub fn evidence() -> Result<(State, Effective), Box<dyn std::error::Error>> {
    let effective = MonitorConfig::parse(
        "version=1\n[[targets]]\nname='fixture'\nprovider='edge'\nscope='fixture'",
    )?
    .resolve(&Default::default())?;
    let job = effective
        .jobs
        .iter()
        .find(|job| job.check == Check::Edge)
        .ok_or("edge job")?;
    let mut state = State::new(
        effective.revision.clone(),
        effective.jobs.iter().map(|job| job.key.clone()).collect(),
    );
    let mut result = CheckResult::failure(
        "fixture".into(),
        Check::Edge,
        effective.revision.clone(),
        Coverage::Complete,
    );
    result.operations[0].id = "endpoints".into();
    result.observations.push(Observation {
        resource: "fixture/endpoints/api".into(),
        operation: "endpoints".into(),
        observed_at: chrono::Utc::now(),
        expected: Expected::Active,
        data: Data::Endpoint {
            dns: true,
            tls: true,
            status: Some(503),
            accepted: vec![200],
            latency_ms: 5,
            expires_at: Some(chrono::Utc::now() + chrono::Duration::days(30)),
        },
    });
    state.apply(job, result, chrono::Utc::now());
    Ok((state, effective))
}
pub fn request(path: &str) -> Result<axum::extract::Request, axum::http::Error> {
    axum::http::Request::builder()
        .uri(path)
        .header("host", "localhost")
        .extension(axum::extract::ConnectInfo(crate::listener::Peer(
            std::net::SocketAddr::from(([127, 0, 0, 1], 9000)),
        )))
        .body(axum::body::Body::empty())
}
