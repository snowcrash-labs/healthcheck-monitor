//! Read-only API, static assets, and admission budgets share one supervised application state.
use crate::{bus::Bus, config::Config, response::ApiError, security::Security};
use axum::{Router, extract::DefaultBodyLimit, middleware, routing::get};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
pub struct App {
    pub bus: Arc<Bus>,
    pub history: Arc<monitor_history::History>,
    pub security: Security,
    pub requests: Arc<Semaphore>,
    pub streams: Arc<Semaphore>,
    pub stop: CancellationToken,
    pub response_bytes: usize,
    pub tls: bool,
}
pub fn router(app: Arc<App>, _config: &Config) -> Router {
    let api = Router::new()
        .route("/overview", get(crate::overview::overview))
        .route("/checks", get(crate::checks_api::checks))
        .route("/check", get(crate::checks_api::check))
        .route("/check/operations", get(crate::checks_api::operations))
        .route("/resource/evidence", get(crate::checks_api::evidence))
        .route("/resources", get(crate::lists::resources))
        .route("/resource", get(crate::lists::resource))
        .route("/findings", get(crate::lists::findings))
        .route("/history", get(crate::history_api::events))
        .route("/runs", get(crate::history_api::runs))
        .route("/events", get(crate::events::events))
        .fallback(|| async { ApiError::NotFound });
    Router::new()
        .nest("/api/v1", api)
        .route("/healthz", get(crate::events::health))
        .fallback(crate::static_files::serve)
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(DefaultBodyLimit::max(1024))
        .layer(middleware::from_fn_with_state(
            app.clone(),
            crate::security::guard,
        ))
        .with_state(app)
}
