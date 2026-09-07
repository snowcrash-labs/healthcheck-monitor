//! Historical coverage includes missed observation windows, not only database publication status.
use crate::api::App;
use monitor_query::{
    filter::{Filter, Window},
    response::Availability,
};
pub async fn scoped(app: &App, filter: &Filter, window: Window) -> Availability {
    let mut availability = crate::query_api::availability(app, window).await;
    if !availability.history_available {
        return availability;
    }
    let mut coverage = filter.clone();
    if coverage.target.is_none()
        && let Some(resource) = &filter.resource
    {
        coverage.target = app.bus.current().and_then(|v| {
            v.resources
                .iter()
                .find(|r| &r.id == resource)
                .map(|r| r.target.clone())
        });
    }
    match app.history.query_gap_count(&coverage, window).await {
        Ok(0) => {}
        Ok(_) => {
            availability.complete = false;
            availability.gaps.push(
                "Required checks have failed or missing observation windows in this period".into(),
            );
        }
        Err(_) => {
            availability.complete = false;
            availability
                .gaps
                .push("Historical collection coverage could not be evaluated".into());
        }
    }
    availability
}
