//! Deployment verdicts require observed post-deployment evidence, never successful collection alone.
use crate::{
    enums::*,
    filter::{Deployment, Window},
    record::{Details, Record},
    response::{Assessment, Availability},
};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

pub struct Evidence<'a> {
    pub checks: &'a [Record],
    pub required_checks: &'a [String],
    pub releases: &'a [Record],
    pub release_assessment: Option<crate::release_assessment::ReleaseAssessment>,
    pub findings: &'a [Record],
    pub baseline: &'a [Record],
    pub error_count: u64,
    pub failed_checks: u64,
    pub availability: Availability,
    pub next_cursor: Option<String>,
}
pub fn evaluate(
    query: &Deployment,
    evidence: Evidence<'_>,
    now: DateTime<Utc>,
) -> Result<Assessment, crate::Error> {
    let window = query.window()?;
    let baseline = Window {
        from: window.from - (window.to - window.from),
        to: window.from,
    };
    let mut result = Assessment {
        scope: query.filter.clone(),
        assessed_checks: evidence.required_checks.to_vec(),
        outcome: Outcome::Pending,
        window,
        baseline,
        availability: evidence.availability,
        reasons: vec![],
        outstanding_checks: vec![],
        next_observation_at: None,
        new_findings: vec![],
        worsened_findings: vec![],
        pre_existing_findings: vec![],
        recovered_findings: vec![],
        unclassified_findings: vec![],
        next_cursor: evidence.next_cursor,
        error_count: evidence.error_count,
        failed_check_count: evidence.failed_checks,
    };
    let mut baseline_severity = BTreeMap::new();
    for r in evidence.baseline {
        if r.closed_at.is_some_and(|at| at < window.from) {
            continue;
        }
        if let Some(severity) = r.severity() {
            baseline_severity
                .entry(r.identity.as_str())
                .and_modify(|s: &mut Severity| *s = (*s).max(severity))
                .or_insert(severity);
        }
    }
    for record in evidence.findings {
        if record.state() == Some(FindingState::Recovered)
            && record
                .closed_at
                .is_some_and(|at| at >= window.from && at < window.to)
        {
            result.recovered_findings.push(record.clone());
        }
        let old = baseline_severity.get(record.identity.as_str()).copied();
        let earlier = matches!(&record.details,Details::Finding {first_detected_at:Some(at),..} if *at<window.from);
        let started = matches!(&record.details,Details::Finding {first_detected_at:Some(at),..} if *at>=window.from&&*at<window.to);
        if started {
            result.new_findings.push(record.clone());
        } else if old.is_some_and(|s| record.severity().is_some_and(|severity| severity > s)) {
            result.worsened_findings.push(record.clone());
        } else if old.is_some() || earlier {
            result.pre_existing_findings.push(record.clone());
        } else {
            result.unclassified_findings.push(record.clone());
        }
    }
    let mut checks: BTreeMap<&str, &Record> = BTreeMap::new();
    for record in evidence.checks {
        if record.observed_at < window.to {
            let old = checks.get(record.identity.as_str());
            if old.is_none_or(|old| old.observed_at < record.observed_at) {
                checks.insert(&record.identity, record);
            }
        }
    }
    for required in evidence.required_checks {
        let complete = checks
            .get(required.as_str())
            .is_some_and(|record| match &record.details {
                Details::Check {
                    started_at,
                    finished_at,
                    oldest_observation_at,
                    complete,
                    pending_observations,
                    operations_truncated,
                    ..
                } => {
                    *complete
                        && !*operations_truncated
                        && *pending_observations == 0
                        && *started_at >= window.from
                        && *finished_at >= window.from
                        && oldest_observation_at.is_none_or(|at| at >= window.from)
                        && record
                            .valid_until
                            .is_some_and(|at| at >= window.to.min(now))
                        && !record.stale
                }
                _ => false,
            });
        if !complete {
            result.outstanding_checks.push(required.clone());
        }
        if let Some(record) = checks.get(required.as_str())
            && let Details::Check {
                interval_seconds, ..
            } = record.details
        {
            let next = record.last_observed_at + chrono::Duration::seconds(interval_seconds as i64);
            if next > now {
                result.next_observation_at =
                    Some(result.next_observation_at.map_or(next, |old| old.min(next)));
            }
        }
    }
    if evidence.required_checks.is_empty() {
        result
            .reasons
            .push("No configured checks match this deployment scope".into());
    }
    if !result.outstanding_checks.is_empty() {
        result.reasons.push("Required checks lack complete fresh post-deployment evidence or still have unevaluated observations".into());
    }
    if evidence.failed_checks > 0 {
        result
            .reasons
            .push("Collection failures occurred inside the deployment window".into());
    }
    let mut revision_mismatch = false;
    if query.expected_revision.is_some() || query.expected_digest.is_some() {
        let status = match evidence.release_assessment {
            Some(status) => status,
            None => {
                let mut status = crate::release_assessment::ReleaseAssessment::default();
                for record in evidence.releases {
                    status.observe(query, record, window, now);
                }
                status
            }
        };
        revision_mismatch = status.mismatch;
        result.reasons.extend(status.reasons());
    }
    if evidence.error_count > 0 {
        result
            .reasons
            .push("Error-level findings were observed in the deployment window".into());
    }
    result.reasons.sort();
    result.reasons.dedup();
    result.outcome = if evidence.error_count > 0 || revision_mismatch && now >= window.to {
        if revision_mismatch {
            result
                .reasons
                .push("Observed release does not match the requested revision or digest".into());
        }
        Outcome::Failing
    } else if now < window.to {
        Outcome::Pending
    } else if !result.availability.complete || !result.reasons.is_empty() {
        Outcome::Incomplete
    } else {
        Outcome::Passing
    };
    Ok(result)
}
