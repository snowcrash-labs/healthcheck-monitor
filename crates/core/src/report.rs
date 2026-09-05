//! Offline summaries and structural comparisons ignore incidental sample values.
use crate::model::*;
use std::collections::BTreeSet;
/// Coverage has precedence over health errors in one-off exit status.
pub fn exit_code(snapshot: &Snapshot, strict: bool) -> u8 {
    if snapshot.persistence_fault {
        return 2;
    }
    if snapshot
        .selected_scope
        .iter()
        .any(|key| snapshot.results.get(key).is_none_or(|r| !r.complete()))
    {
        return 3;
    }
    if snapshot
        .findings
        .values()
        .any(|f| f.severity == Severity::Error || strict && f.severity == Severity::Warning)
    {
        return 1;
    }
    0
}
fn safe(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect::<String>()
        .replace(['|', '<', '>', '`'], "_")
}
pub fn markdown(snapshot: &Snapshot) -> String {
    let mut out = format!(
        "# Health check\n\nObserved: {}\n\nSelected scope: {} checks. This report covers only the selected scope.\n\n| Check | Collection coverage | Observations |\n| --- | --- | --- |\n",
        snapshot.captured_at.to_rfc3339(),
        snapshot.selected_scope.len()
    );
    for key in &snapshot.selected_scope {
        match snapshot.results.get(key) {
            Some(result) => {
                let coverage: BTreeSet<_> = result
                    .operations
                    .iter()
                    .map(|o| format!("{:?}", o.coverage))
                    .collect();
                out.push_str(&format!(
                    "| {} | {} | {} |\n",
                    safe(key),
                    coverage.into_iter().collect::<Vec<_>>().join(", "),
                    result.observations.len()
                ));
            }
            None => out.push_str(&format!("| {} | Missing | 0 |\n", safe(key))),
        }
    }
    out.push_str("\n| Resource | Rule | Severity | Stale |\n| --- | --- | --- | --- |\n");
    for finding in snapshot.findings.values() {
        out.push_str(&format!(
            "| {} | {} | {:?} | {} |\n",
            safe(&finding.resource),
            safe(&finding.rule),
            finding.severity,
            finding.stale
        ));
    }
    if snapshot.findings.is_empty() {
        out.push_str("\nNo active findings. Inventory and missing telemetry do not establish system health.\n");
    }
    if snapshot.persistence_fault {
        out.push_str("\nPersistence fault: some history could not be published.\n");
    }
    out
}
pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Transition> {
    let mut transitions = Vec::new();
    for (id, finding) in &new.findings {
        let kind = match old.findings.get(id) {
            None => Some(TransitionKind::New),
            Some(prior) if !prior.stale && finding.stale => Some(TransitionKind::Stale),
            Some(prior) if prior.stale && !finding.stale => Some(TransitionKind::Reappeared),
            Some(prior) if finding.severity > prior.severity => Some(TransitionKind::Worsened),
            _ => None,
        };
        if let Some(kind) = kind {
            transitions.push(Transition {
                at: new.captured_at,
                finding: id.clone(),
                kind,
            });
        }
    }
    // Offline disappearance is not proof of recovery, particularly across scope changes.
    for id in old
        .findings
        .keys()
        .filter(|id| !new.findings.contains_key(*id))
    {
        transitions.push(Transition {
            at: new.captured_at,
            finding: id.clone(),
            kind: TransitionKind::Removed,
        });
    }
    transitions
}
