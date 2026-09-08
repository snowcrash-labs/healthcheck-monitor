//! Billing requests read published local aggregates and never launch provider queries.
use crate::{
    api::App,
    response::{ApiError, json},
};
use axum::{
    extract::{Query, State},
    response::Response,
};
use monitor_costs::{aggregate::View, query::Filter};
use std::sync::Arc;

pub async fn view(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    let period = filter.period().map_err(|_| ApiError::BadQuery)?;
    let _permit = app
        .cost_requests
        .try_acquire()
        .map_err(|_| ApiError::Busy)?;
    if !app.costs.enabled {
        return json(
            &View {
                enabled: false,
                revision: String::new(),
                period,
                currency: "USD".into(),
                measure: filter.measure,
                group: filter.group,
                granularity: filter.granularity,
                total: None,
                previous_total: None,
                complete: false,
                sources: vec![],
                series: vec![],
                breakdown: vec![],
                next_cursor: None,
                contributor_count: 0,
            },
            app.response_bytes,
        );
    }
    let view = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        app.history.cost_view(&filter, &app.costs),
    )
    .await
    .map_err(|_| ApiError::Unavailable)??;
    json(&view, app.response_bytes)
}
pub async fn sources(State(app): State<Arc<App>>) -> Result<Response, ApiError> {
    if !app.costs.enabled {
        return json(
            &Vec::<monitor_costs::model::SourceStatus>::new(),
            app.response_bytes,
        );
    }
    let sources = app
        .history
        .cost_status()
        .await?
        .into_iter()
        .filter(|s| app.costs.sources.iter().any(|c| c.id == s.id))
        .collect::<Vec<_>>();
    json(&sources, app.response_bytes)
}
