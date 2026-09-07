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
    let probe = Filter {
        target: coverage.target.clone(),
        provider: coverage.provider,
        scope: coverage.scope.clone(),
        check: coverage.check,
        limit: Some(1),
        ..Default::default()
    };
    match app
        .history
        .query_page(
            &probe,
            Window {
                from: window.to - chrono::Duration::days(31),
                to: window.to,
            },
            Some(monitor_query::enums::Category::Check),
            None,
        )
        .await
    {
        Ok(rows) if rows.is_empty() => {
            availability.complete = false;
            availability.gaps.push(
                "No collected checks establish coverage for the selected scope and period".into(),
            );
        }
        Ok(_) => {}
        Err(_) => {
            availability.complete = false;
            availability
                .gaps
                .push("Check coverage for the selected scope could not be established".into());
        }
    }
    if window.to >= chrono::Utc::now() - chrono::Duration::seconds(30)
        && app.bus.current().is_some_and(|view| {
            view.checks.iter().any(|check| {
                check.required
                    && check.finished_at.is_none()
                    && probe
                        .check
                        .is_none_or(|c| crate::query_projection::check(check.check) == c)
                    && view
                        .targets
                        .iter()
                        .find(|t| t.name == check.target)
                        .is_some_and(|t| {
                            monitor_query::matching::scope(
                                &probe,
                                &crate::query_projection::scope(t),
                                &Default::default(),
                            )
                        })
            })
        })
    {
        availability.complete = false;
        availability
            .gaps
            .push("Some configured required checks have not produced observations yet".into());
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
