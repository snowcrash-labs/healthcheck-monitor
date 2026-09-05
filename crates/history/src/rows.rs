//! Typed query and insertion models omit all database-generated primary keys on writes.
use crate::{
    enums, records,
    schema::{check_run, configuration, finding_event, history_gap},
    types::{Digest, Evidence, Id, Name, Resource},
};
use chrono::{DateTime, Utc};
use diesel::{Insertable, Queryable, Selectable};
use serde::Serialize;

#[derive(Insertable)]
#[diesel(table_name = configuration)]
pub(crate) struct NewConfiguration {
    pub configuration_revision: Digest,
}
#[derive(Insertable)]
#[diesel(table_name = check_run)]
pub(crate) struct NewRun {
    pub check_run_configuration_id: Id,
    pub check_run_key: Digest,
    pub check_run_target: Name,
    pub check_run_check: enums::Check,
    pub check_run_started_at: DateTime<Utc>,
    pub check_run_finished_at: DateTime<Utc>,
    pub check_run_complete: bool,
    pub check_run_observations: i64,
    pub check_run_required_failures: i64,
}
impl NewRun {
    pub fn new(configuration: Id, run: records::Run) -> Self {
        Self {
            check_run_configuration_id: configuration,
            check_run_key: run.key,
            check_run_target: run.target,
            check_run_check: run.check,
            check_run_started_at: run.started_at,
            check_run_finished_at: run.finished_at,
            check_run_complete: run.complete,
            check_run_observations: run.observations,
            check_run_required_failures: run.failures,
        }
    }
}
#[derive(Insertable)]
#[diesel(table_name = finding_event)]
pub(crate) struct NewEvent {
    pub finding_event_configuration_id: Id,
    pub finding_event_key: Digest,
    pub finding_event_target: Name,
    pub finding_event_finding: Resource,
    pub finding_event_resource: Resource,
    pub finding_event_rule: Name,
    pub finding_event_kind: enums::Kind,
    pub finding_event_severity: enums::Severity,
    pub finding_event_expected: enums::Expected,
    pub finding_event_confidence: enums::Confidence,
    pub finding_event_observed_at: DateTime<Utc>,
    pub finding_event_at: DateTime<Utc>,
    pub finding_event_stale: bool,
    pub finding_event_evidence: Vec<Evidence>,
}
impl NewEvent {
    pub fn new(configuration: Id, event: records::Event) -> Self {
        Self {
            finding_event_configuration_id: configuration,
            finding_event_key: event.key,
            finding_event_target: event.target,
            finding_event_finding: event.finding,
            finding_event_resource: event.resource,
            finding_event_rule: event.rule,
            finding_event_kind: event.kind,
            finding_event_severity: event.severity,
            finding_event_expected: event.expected,
            finding_event_confidence: event.confidence,
            finding_event_observed_at: event.observed_at,
            finding_event_at: event.at,
            finding_event_stale: event.stale,
            finding_event_evidence: event.evidence,
        }
    }
}
#[derive(Insertable)]
#[diesel(table_name = history_gap)]
pub(crate) struct NewGap {
    pub history_gap_configuration_id: Id,
    pub history_gap_key: Digest,
    pub history_gap_at: DateTime<Utc>,
    pub history_gap_events: i64,
    pub history_gap_runs: i64,
}
impl NewGap {
    pub fn new(configuration: Id, gap: records::Gap) -> Self {
        Self {
            history_gap_configuration_id: configuration,
            history_gap_key: gap.key,
            history_gap_at: gap.at,
            history_gap_events: gap.events,
            history_gap_runs: gap.runs,
        }
    }
}
#[derive(Debug, Queryable, Selectable, Serialize)]
#[diesel(table_name = finding_event, check_for_backend(diesel::pg::Pg))]
pub struct EventRow {
    #[serde(rename = "id")]
    pub finding_event_id: Id,
    #[serde(rename = "configuration_id")]
    pub finding_event_configuration_id: Id,
    #[serde(skip)]
    pub finding_event_key: Digest,
    #[serde(rename = "target")]
    pub finding_event_target: Name,
    #[serde(rename = "finding")]
    pub finding_event_finding: Resource,
    #[serde(rename = "resource")]
    pub finding_event_resource: Resource,
    #[serde(rename = "rule")]
    pub finding_event_rule: Name,
    #[serde(rename = "kind")]
    pub finding_event_kind: enums::Kind,
    #[serde(rename = "severity")]
    pub finding_event_severity: enums::Severity,
    #[serde(rename = "expected")]
    pub finding_event_expected: enums::Expected,
    #[serde(rename = "confidence")]
    pub finding_event_confidence: enums::Confidence,
    #[serde(rename = "observed_at")]
    pub finding_event_observed_at: DateTime<Utc>,
    #[serde(rename = "at")]
    pub finding_event_at: DateTime<Utc>,
    #[serde(rename = "stale")]
    pub finding_event_stale: bool,
    #[serde(rename = "evidence")]
    pub finding_event_evidence: Vec<Evidence>,
}
#[derive(Debug, Queryable, Selectable, Serialize)]
#[diesel(table_name = check_run, check_for_backend(diesel::pg::Pg))]
pub struct RunRow {
    #[serde(rename = "id")]
    pub check_run_id: Id,
    #[serde(rename = "configuration_id")]
    pub check_run_configuration_id: Id,
    #[serde(skip)]
    pub check_run_key: Digest,
    #[serde(rename = "target")]
    pub check_run_target: Name,
    #[serde(rename = "check")]
    pub check_run_check: enums::Check,
    #[serde(rename = "started_at")]
    pub check_run_started_at: DateTime<Utc>,
    #[serde(rename = "finished_at")]
    pub check_run_finished_at: DateTime<Utc>,
    #[serde(rename = "complete")]
    pub check_run_complete: bool,
    #[serde(rename = "observations")]
    pub check_run_observations: i64,
    #[serde(rename = "required_failures")]
    pub check_run_required_failures: i64,
}
