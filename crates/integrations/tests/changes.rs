//! Deployment comparisons identify workloads without retaining configuration payloads.
use monitor_core::config::types::ChangeScope;
use monitor_integrations::changes::{affected, manifest_workload};
use std::collections::BTreeMap;
#[test]
fn only_changed_configured_paths_select_workloads() {
    let scope = ChangeScope {
        repository: "soundpatrol/backend".into(),
        base: "before".into(),
        head: "after".into(),
        paths: vec!["helm/".into()],
        workloads: BTreeMap::from([
            ("helm/worker/".into(), "transcription/worker".into()),
            ("helm/api/".into(), "transcription/api".into()),
        ]),
    };
    let (workloads, unresolved) = affected(
        &scope,
        &["helm/worker/values.yaml".into(), "docs/guide.md".into()],
    );
    assert_eq!(workloads, vec!["transcription/worker"]);
    assert!(unresolved.is_empty());
}
#[test]
fn ambiguous_template_mapping_stays_unresolved() {
    let scope = ChangeScope {
        repository: "soundpatrol/backend".into(),
        base: "before".into(),
        head: "after".into(),
        paths: vec!["helm/".into()],
        workloads: BTreeMap::new(),
    };
    let (workloads, unresolved) = affected(&scope, &["helm/templates/worker.yaml".into()]);
    assert!(workloads.is_empty());
    assert_eq!(unresolved, vec!["helm/templates/worker.yaml"]);
}
#[test]
fn structural_manifest_parser_returns_only_workload_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let input = "kind: StatefulSet\nmetadata:\n  name: audio-download\n  namespace: transcription\nspec:\n  template:\n    spec:\n      containers:\n      - name: worker\n        env:\n        - name: PASSWORD\n          value: private-value";
    assert_eq!(
        manifest_workload(input)?.as_deref(),
        Some("transcription/audio-download")
    );
    assert_eq!(
        manifest_workload(
            "kind: Secret\nmetadata:\n name: credentials\ndata:\n password: private-value"
        )?,
        None
    );
    Ok(())
}
