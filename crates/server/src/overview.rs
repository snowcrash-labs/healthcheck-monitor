//! Overview health is computed independently of coverage and database availability.
use crate::{
    api::App,
    response::{ApiError, json},
    view::{CheckView, Target, View},
};
use axum::{
    extract::{Query, State},
    response::Response,
};
use monitor_core::model::{Health, Severity};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, atomic::Ordering};
#[derive(Default, Deserialize)]
pub struct Selection {
    pub target: Option<String>,
}
#[derive(Serialize)]
struct TargetSummary {
    #[serde(flatten)]
    target: Target,
    health: Health,
    complete_checks: usize,
    total_checks: usize,
    errors: usize,
    warnings: usize,
    resources: usize,
    latest_observation: Option<chrono::DateTime<chrono::Utc>>,
}
#[derive(Serialize)]
struct Totals {
    targets: usize,
    resources: usize,
    error_findings: usize,
    warning_findings: usize,
    incomplete_checks: usize,
}
#[derive(Serialize)]
struct Overview<'a> {
    total_problem_groups: usize,
    problem_groups: Vec<crate::problem_groups::Group<'a>>,
    generation: u64,
    configuration_revision: &'a str,
    captured_at: chrono::DateTime<chrono::Utc>,
    heartbeat_at: Option<chrono::DateTime<chrono::Utc>>,
    running: bool,
    persistence_fault: bool,
    history: monitor_history::journal::Health,
    totals: Totals,
    targets: Vec<TargetSummary>,
    checks: Vec<CheckView>,
}
pub async fn overview(
    State(app): State<Arc<App>>,
    Query(selection): Query<Selection>,
) -> Result<Response, ApiError> {
    let view = app.bus.current().ok_or(ApiError::Waiting)?;
    if selection
        .target
        .as_ref()
        .is_some_and(|target| !view.targets.iter().any(|row| &row.name == target))
    {
        return Err(ApiError::BadQuery);
    }
    let now = chrono::Utc::now();
    let targets: Vec<_> = view
        .targets
        .iter()
        .map(|target| summary(&view, target, now))
        .collect();
    let selected = |name: &str| {
        selection
            .target
            .as_deref()
            .is_none_or(|target| target == name)
    };
    let totals = Totals {
        targets: targets
            .iter()
            .filter(|target| selected(&target.target.name))
            .count(),
        resources: targets
            .iter()
            .filter(|target| selected(&target.target.name))
            .map(|target| target.resources)
            .sum(),
        error_findings: targets
            .iter()
            .filter(|target| selected(&target.target.name))
            .map(|target| target.errors)
            .sum(),
        warning_findings: targets
            .iter()
            .filter(|target| selected(&target.target.name))
            .map(|target| target.warnings)
            .sum(),
        incomplete_checks: targets
            .iter()
            .filter(|target| selected(&target.target.name))
            .map(|target| target.total_checks - target.complete_checks)
            .sum(),
    };
    let checks = view
        .checks
        .iter()
        .filter(|check| selected(&check.target))
        .cloned()
        .map(|mut check| {
            if check.expires_at.is_some_and(|at| now > at) {
                check.complete = false;
            }
            check
        })
        .collect();
    let (total_problem_groups, problem_groups) =
        crate::problem_groups::groups(&view.findings, selection.target.as_deref());
    json(
        &Overview {
            total_problem_groups,
            problem_groups,
            generation: view.generation,
            configuration_revision: &view.revision,
            captured_at: view.captured_at,
            heartbeat_at: chrono::DateTime::from_timestamp_millis(
                app.bus.heartbeat.load(Ordering::Acquire),
            ),
            running: app.bus.running.load(Ordering::Acquire),
            persistence_fault: view.persistence_fault,
            history: app.bus.journal.health(),
            totals,
            targets,
            checks,
        },
        app.response_bytes,
    )
}
fn summary(view: &View, target: &Target, now: chrono::DateTime<chrono::Utc>) -> TargetSummary {
    let checks: Vec<_> = view
        .checks
        .iter()
        .filter(|check| check.target == target.name)
        .collect();
    let complete = checks
        .iter()
        .filter(|check| check.complete && check.expires_at.is_some_and(|at| at >= now))
        .count();
    let findings: Vec<_> = view
        .findings
        .iter()
        .filter(|finding| finding.target == target.name)
        .collect();
    let errors = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Error)
        .count();
    let warnings = findings
        .iter()
        .filter(|finding| finding.severity == Severity::Warning)
        .count();
    let resources: Vec<_> = view
        .resources
        .iter()
        .filter(|resource| resource.target == target.name)
        .collect();
    let has_health = |health| {
        resources.iter().any(|resource| {
            crate::view::current_health(resource.health, resource.expires_at, now) == health
        })
    };
    let health = if errors > 0 || has_health(Health::Unhealthy) {
        Health::Unhealthy
    } else if warnings > 0 || has_health(Health::Degraded) {
        Health::Degraded
    } else if complete != checks.len()
        || checks.is_empty()
        || resources.is_empty()
        || has_health(Health::Unknown)
    {
        Health::Unknown
    } else if !resources.is_empty()
        && resources
            .iter()
            .all(|resource| resource.health == Health::ExpectedInactive)
    {
        Health::ExpectedInactive
    } else {
        Health::Healthy
    };
    TargetSummary {
        target: target.clone(),
        health,
        complete_checks: complete,
        total_checks: checks.len(),
        errors,
        warnings,
        resources: resources.len(),
        latest_observation: resources.iter().map(|resource| resource.observed_at).max(),
    }
}
