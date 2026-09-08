//! Check and operation drill-downs reuse the published view with keyset pagination.
use crate::{
    api::App,
    pages::{self, Direction, Keyed, Page},
    response::{ApiError, json},
    view::CheckView,
};
use axum::{
    extract::{Query, State},
    response::Response,
};
use monitor_core::model::{Check, Operation};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub target: Option<String>,
    pub check: Option<Check>,
    pub id: Option<String>,
    pub q: Option<String>,
    pub status: Option<String>,
    pub cursor: Option<String>,
    #[serde(default)]
    pub direction: Direction,
    pub limit: Option<usize>,
}
impl Filter {
    fn limit(&self) -> Result<usize, ApiError> {
        let limit = self.limit.unwrap_or(50);
        if !(1..=100).contains(&limit)
            || self.q.as_ref().is_some_and(|s| s.len() > 128)
            || self.target.as_ref().is_some_and(|s| s.len() > 128)
            || self.id.as_ref().is_some_and(|s| s.len() > 4096)
            || self.status.as_ref().is_some_and(|s| {
                ![
                    "complete",
                    "incomplete",
                    "stale",
                    "awaiting",
                    "needs_attention",
                ]
                .contains(&s.as_str())
            })
        {
            return Err(ApiError::BadQuery);
        }
        Ok(limit)
    }
    fn matches(&self, row: &CheckView) -> bool {
        self.target.as_ref().is_none_or(|t| *t == row.target)
            && self.check.is_none_or(|c| c == row.check)
    }
}
impl Keyed for CheckView {
    fn key(&self) -> (u8, &str, &str) {
        (u8::from(self.complete), &self.key, "")
    }
}
impl Keyed for Operation {
    fn key(&self) -> (u8, &str, &str) {
        (
            u8::from(self.coverage == monitor_core::model::Coverage::Complete),
            &self.id,
            "",
        )
    }
}
fn page<T: Keyed + Clone + serde::Serialize>(
    rows: Vec<&T>,
    filter: &Filter,
    generation: u64,
    bytes: usize,
) -> Result<Response, ApiError> {
    let selected = pages::select(
        rows,
        filter.cursor.as_deref(),
        &filter.direction,
        filter.limit()?,
    )?;
    json(
        &Page {
            generation,
            items: selected.items,
            next_cursor: selected.next,
            previous_cursor: selected.previous,
            total: selected.total,
        },
        bytes,
    )
}
pub async fn checks(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    filter.limit()?;
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let query = filter.q.as_deref().unwrap_or("").to_lowercase();
    let now = chrono::Utc::now();
    let rows = view
        .checks
        .iter()
        .filter(|row| {
            let state = if row.finished_at.is_none() {
                "awaiting"
            } else if row.expires_at.is_some_and(|at| at < now) {
                "stale"
            } else if row.complete {
                "complete"
            } else {
                "incomplete"
            };
            filter.matches(row)
                && (row.key.to_lowercase().contains(&query)
                    || label(row.check).to_lowercase().contains(&query))
                && filter
                    .status
                    .as_deref()
                    .is_none_or(|s| s == state || s == "needs_attention" && state != "complete")
        })
        .cloned()
        .map(|mut row| {
            row.complete &= row.expires_at.is_some_and(|at| at >= now);
            row
        })
        .collect::<Vec<_>>();
    page(
        rows.iter().collect(),
        &filter,
        view.generation,
        app.response_bytes,
    )
}
pub async fn check(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    filter.limit()?;
    if filter.target.is_none() || filter.check.is_none() {
        return Err(ApiError::BadQuery);
    }
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let row = view
        .checks
        .iter()
        .find(|row| filter.matches(row))
        .ok_or(ApiError::NotFound)?;
    json(row, app.response_bytes)
}
pub async fn operations(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    filter.limit()?;
    if filter.target.is_none() || filter.check.is_none() {
        return Err(ApiError::BadQuery);
    }
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let row = view
        .checks
        .iter()
        .find(|row| filter.matches(row))
        .ok_or(ApiError::NotFound)?;
    let query = filter.q.as_deref().unwrap_or("").to_lowercase();
    let rows = row
        .operations
        .iter()
        .filter(|op| op.id.to_lowercase().contains(&query))
        .collect();
    page(rows, &filter, view.generation, app.response_bytes)
}
pub async fn evidence(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Response, ApiError> {
    filter.limit()?;
    let id = filter.id.as_deref().ok_or(ApiError::BadQuery)?;
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    let row = view
        .resources
        .binary_search_by(|r| r.id.as_str().cmp(id))
        .ok()
        .and_then(|i| view.resources.get(i))
        .ok_or(ApiError::NotFound)?;
    let query = filter.q.as_deref().unwrap_or("").to_lowercase();
    let rows = row
        .evidence
        .iter()
        .filter(|e| {
            e.operation.to_lowercase().contains(&query)
                && filter.check.is_none_or(|check| e.check == check)
        })
        .collect();
    page(rows, &filter, view.generation, app.response_bytes)
}

fn label(check: Check) -> &'static str {
    match check {
        Check::Preflight => "Access and prerequisites",
        Check::Discovery => "Organization discovery",
        Check::Inventory => "Resource inventory",
        Check::Kubernetes => "Kubernetes health",
        Check::Edge => "DNS, TLS, and endpoints",
        Check::Managed => "Managed dependencies",
        Check::Queues => "Queues and consumers",
        Check::Releases => "Release verification",
        Check::Github => "Repository and workflow checks",
        Check::Metrics => "Performance and capacity",
        Check::Logs => "Runtime diagnostics",
        Check::Alerts => "Provider alerts and incidents",
        Check::Slo => "Service objectives",
        Check::Flows => "Processing progress",
    }
}
