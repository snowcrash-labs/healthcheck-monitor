//! Join one resource page to finding summaries without extra database or provider requests.
use crate::view::{FindingView, Resource};
use chrono::{DateTime, Utc};
use monitor_core::model::Severity;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Row {
    #[serde(flatten)]
    resource: Resource,
    finding_count: usize,
    findings: Vec<Preview>,
}
#[derive(Serialize)]
struct Preview {
    rule: String,
    severity: Severity,
    observed_at: DateTime<Utc>,
    valid_until: Option<DateTime<Utc>>,
    stale: bool,
    diagnostic: Option<crate::resource_evidence::DiagnosticView>,
}
/// Retain two highest-severity summaries per row; count every matching active finding.
pub fn with_findings(
    resources: Vec<Resource>,
    findings: &[FindingView],
    now: DateTime<Utc>,
) -> Vec<Row> {
    let indices: BTreeMap<_, _> = resources
        .iter()
        .enumerate()
        .map(|(index, resource)| (resource.id.clone(), index))
        .collect();
    let mut rows: Vec<_> = resources
        .into_iter()
        .map(|resource| Row {
            resource,
            finding_count: 0,
            findings: Vec::with_capacity(3),
        })
        .collect();
    for finding in findings {
        let Some(row) = indices
            .get(&finding.resource)
            .and_then(|index| rows.get_mut(*index))
        else {
            continue;
        };
        row.finding_count += 1;
        row.findings.push(Preview {
            diagnostic: finding.diagnostic.clone(),
            rule: finding.rule.clone(),
            severity: finding.severity,
            observed_at: finding.observed_at,
            valid_until: finding.valid_until,
            stale: finding.stale || finding.valid_until.is_some_and(|at| now > at),
        });
        row.findings.sort_unstable_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.rule.cmp(&b.rule))
        });
        row.findings.truncate(2);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summaries_prioritize_errors_preserve_age_and_count_truncation()
    -> Result<(), Box<dyn std::error::Error>> {
        let (state, effective) = crate::test_support::evidence()?;
        let view = crate::build_view::build(&state.snapshot, &effective, 1);
        let finding = view.findings.first().ok_or("fixture finding")?;
        let now = Utc::now();
        let mut findings = Vec::new();
        for (rule, severity) in [
            ("informational", Severity::Info),
            ("warning", Severity::Warning),
            ("error", Severity::Error),
        ] {
            let mut row = finding.clone();
            row.rule = rule.into();
            row.severity = severity;
            row.valid_until = Some(now - chrono::Duration::seconds(1));
            findings.push(row);
        }
        let mut unrelated = finding.clone();
        unrelated.resource = "another/resource".into();
        findings.push(unrelated);
        let rows = with_findings(view.resources, &findings, now);
        let row = rows.first().ok_or("resource row")?;
        assert_eq!(row.finding_count, 3);
        assert_eq!(row.findings.len(), 2);
        assert_eq!(row.findings[0].rule, "error");
        assert_eq!(row.findings[1].rule, "warning");
        assert!(
            row.findings
                .iter()
                .all(|row| row.stale && row.observed_at == finding.observed_at)
        );
        Ok(())
    }
}
