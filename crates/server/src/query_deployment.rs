//! Deployment assessment combines historical diagnostics with explicitly selected check coverage.
use crate::{
    api::App,
    response::{ApiError, json},
};
use axum::{
    extract::{Query, State},
    response::Response,
};
use monitor_query::{
    assessment::{Evidence, evaluate},
    enums::{Category, Severity},
    filter::{Deployment, Window},
};
use std::sync::Arc;

pub async fn deployment(
    State(app): State<Arc<App>>,
    Query(mut query): Query<Deployment>,
) -> Result<Response, ApiError> {
    query.filter.normalize().map_err(|_| ApiError::BadQuery)?;
    let window = query.window().map_err(|_| ApiError::BadQuery)?;
    let now = chrono::Utc::now();
    if query.deployed_at > now {
        return Err(ApiError::BadQuery);
    }
    let baseline = Window {
        from: window.from - (window.to - window.from),
        to: window.from,
    };
    let mut availability = crate::query_api::availability(
        &app,
        Window {
            from: baseline.from,
            to: window.to.min(now),
        },
    )
    .await;
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let target = if let Some(resource) = &query.filter.resource {
        view.resources
            .iter()
            .find(|r| &r.id == resource)
            .map(|r| r.target.clone())
    } else {
        None
    };
    let mut check_filter = query.filter.clone();
    check_filter.resource = None;
    check_filter.region = None;
    check_filter.namespace = None;
    check_filter.cluster = None;
    check_filter.service = None;
    check_filter.hostname = None;
    check_filter.severity = None;
    check_filter.state = None;
    check_filter.q = None;
    if target.is_some() {
        check_filter.target = target;
    }
    match app
        .history
        .query_gap_count(
            &check_filter,
            Window {
                from: baseline.from,
                to: window.to.min(now),
            },
        )
        .await
    {
        Ok(0) => {}
        Ok(_) => {
            availability.complete = false;
            availability.gaps.push(
                "The deployment or baseline period contains missing required observations".into(),
            );
        }
        Err(_) => {
            availability.complete = false;
            availability
                .gaps
                .push("Historical collection coverage could not be evaluated".into());
        }
    }
    let required: Vec<_> = view
        .checks
        .iter()
        .filter(|c| {
            c.required
                && query
                    .filter
                    .check
                    .is_none_or(|check| crate::query_projection::check(c.check) == check)
                && view
                    .targets
                    .iter()
                    .find(|t| t.name == c.target)
                    .is_some_and(|t| {
                        monitor_query::matching::scope(
                            &check_filter,
                            &crate::query_projection::scope(t),
                            &Default::default(),
                        )
                    })
        })
        .map(|c| c.key.clone())
        .collect();
    let observed = Window {
        from: baseline.from,
        to: window.to.min(now),
    };
    let checks =
        crate::query_deployment_pages::checks(&app, &check_filter, observed, &required).await?;
    let release_assessment =
        crate::query_deployment_pages::releases(&app, &query, window, now).await?;
    let mut finding_filter = query.filter.clone();
    finding_filter.state = None;
    finding_filter.severity = None;
    finding_filter.from = Some(window.from);
    finding_filter.to = Some(window.to.min(now));
    finding_filter.limit = Some(50);
    let page = crate::query_api::page(
        &app,
        finding_filter.clone(),
        "findings",
        Some(Category::Finding),
    )
    .await?;
    let identities: Vec<_> = page.items.iter().map(|r| r.identity.clone()).collect();
    let baseline_rows = app
        .history
        .query_latest(
            &finding_filter,
            baseline,
            Category::Finding,
            None,
            Some(&identities),
        )
        .await?;
    let error_count = app
        .history
        .query_count(
            &finding_filter,
            window,
            Category::Finding,
            Some(Severity::Error),
        )
        .await? as u64;
    let failed_checks = app
        .history
        .query_failed_checks(&check_filter, window)
        .await? as u64;
    let result = evaluate(
        &query,
        Evidence {
            checks: &checks,
            required_checks: &required,
            releases: &[],
            release_assessment: Some(release_assessment),
            findings: &page.items,
            baseline: &baseline_rows,
            error_count,
            failed_checks,
            availability,
            next_cursor: page.next_cursor,
        },
        now,
    )
    .map_err(|_| ApiError::BadQuery)?;
    json(&result, app.response_bytes)
}
