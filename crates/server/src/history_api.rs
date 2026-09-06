//! Historical events use validated filters and PostgreSQL keyset cursors.
use crate::{
    api::App,
    response::{ApiError, json},
};
use axum::{
    extract::{Query, State},
    response::Response,
};
use monitor_history::{
    enums,
    query::Filter,
    types::{Id, Name, Resource},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    target: Option<String>,
    check: Option<enums::Check>,
    resource: Option<String>,
    severity: Option<enums::Severity>,
    kind: Option<enums::Kind>,
    before: Option<String>,
    limit: Option<u16>,
}
#[derive(Serialize)]
struct Page<T> {
    items: Vec<T>,
    next_cursor: Option<String>,
    gaps: i64,
}
fn cursor(value: Option<&str>) -> Result<Option<(chrono::DateTime<chrono::Utc>, Id)>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > 128 {
        return Err(ApiError::BadQuery);
    }
    let (at, id) = value.split_once('|').ok_or(ApiError::BadQuery)?;
    let at = chrono::DateTime::parse_from_rfc3339(at)
        .map_err(|_| ApiError::BadQuery)?
        .to_utc();
    let id =
        Id::try_new(id.parse().map_err(|_| ApiError::BadQuery)?).map_err(|_| ApiError::BadQuery)?;
    Ok(Some((at, id)))
}
pub async fn events(
    State(app): State<Arc<App>>,
    Query(query): Query<Parameters>,
) -> Result<Response, ApiError> {
    let limit = query.limit.unwrap_or(50);
    if limit == 0 || limit > 100 {
        return Err(ApiError::BadQuery);
    }
    let filter = Filter {
        target: query
            .target
            .map(Name::try_new)
            .transpose()
            .map_err(|_| ApiError::BadQuery)?,
        resource: query
            .resource
            .map(Resource::try_new)
            .transpose()
            .map_err(|_| ApiError::BadQuery)?,
        severity: query.severity,
        kind: query.kind,
        before: cursor(query.before.as_deref())?,
        limit,
    };
    let mut items = app.history.events(&filter).await?;
    let more = items.len() > usize::from(limit);
    items.truncate(usize::from(limit));
    let next_cursor = if more {
        items.last().map(|row| {
            format!(
                "{}|{}",
                row.finding_event_at.to_rfc3339(),
                row.finding_event_id.as_ref()
            )
        })
    } else {
        None
    };
    let gaps = app.history.gaps().await?;
    json(
        &Page {
            items,
            next_cursor,
            gaps,
        },
        app.response_bytes,
    )
}
pub async fn runs(
    State(app): State<Arc<App>>,
    Query(query): Query<Parameters>,
) -> Result<Response, ApiError> {
    let limit = query.limit.unwrap_or(50);
    if limit == 0 || limit > 100 {
        return Err(ApiError::BadQuery);
    }
    let target = query
        .target
        .map(Name::try_new)
        .transpose()
        .map_err(|_| ApiError::BadQuery)?;
    let mut items = app
        .history
        .runs(
            target.as_ref(),
            query.check,
            cursor(query.before.as_deref())?,
            limit,
        )
        .await?;
    let more = items.len() > usize::from(limit);
    items.truncate(usize::from(limit));
    let next_cursor = if more {
        items.last().map(|row| {
            format!(
                "{}|{}",
                row.check_run_finished_at.to_rfc3339(),
                row.check_run_id.as_ref()
            )
        })
    } else {
        None
    };
    let gaps = app.history.gaps().await?;
    json(
        &Page {
            items,
            next_cursor,
            gaps,
        },
        app.response_bytes,
    )
}
