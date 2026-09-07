//! Problem previews aggregate the selected immutable view, independently of pagination.
use crate::view::FindingView;
use chrono::{DateTime, Utc};
use monitor_core::model::{Check, Severity};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
pub struct Group<'a> {
    pub target: &'a str,
    pub rule: &'a str,
    pub check: Option<Check>,
    pub severity: Severity,
    pub resources: usize,
    pub findings: usize,
    pub first_detected_at: Option<DateTime<Utc>>,
    pub last_detected_at: DateTime<Utc>,
    pub stale: bool,
    pub example_resource: &'a str,
}
/// Group by rule and target without asserting a common root cause.
pub fn groups<'a>(findings: &'a [FindingView], target: Option<&str>) -> (usize, Vec<Group<'a>>) {
    let mut groups = BTreeMap::<(&str, &str), (Group<'a>, BTreeSet<&str>)>::new();
    for finding in findings
        .iter()
        .filter(|f| target.is_none_or(|t| t == f.target))
    {
        let first = finding
            .diagnostic
            .as_ref()
            .and_then(|d| d.first_detected_at);
        let last = finding
            .diagnostic
            .as_ref()
            .map_or(finding.observed_at, |d| d.last_detected_at);
        let (group, resources) = groups
            .entry((&finding.target, &finding.rule))
            .or_insert_with(|| {
                (
                    Group {
                        target: &finding.target,
                        rule: &finding.rule,
                        check: finding.check,
                        severity: finding.severity,
                        resources: 0,
                        findings: 0,
                        first_detected_at: first,
                        last_detected_at: last,
                        stale: false,
                        example_resource: &finding.resource,
                    },
                    BTreeSet::new(),
                )
            });
        group.findings += 1;
        resources.insert(&finding.resource);
        group.resources = resources.len();
        if priority(finding.severity) < priority(group.severity) {
            group.severity = finding.severity;
        }
        group.first_detected_at = match (group.first_detected_at, first) {
            (Some(a), Some(b)) => Some(a.min(b)),
            _ => None,
        };
        group.last_detected_at = group.last_detected_at.max(last);
        group.stale |= finding.stale || finding.valid_until.is_some_and(|at| at < Utc::now());
    }
    let total = groups.len();
    let mut values: Vec<_> = groups.into_values().map(|(group, _)| group).collect();
    values.sort_unstable_by(|a, b| {
        (
            priority(a.severity),
            std::cmp::Reverse(a.resources),
            a.target,
            a.rule,
        )
            .cmp(&(
                priority(b.severity),
                std::cmp::Reverse(b.resources),
                b.target,
                b.rule,
            ))
    });
    values.truncate(5);
    (total, values)
}
fn priority(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}
