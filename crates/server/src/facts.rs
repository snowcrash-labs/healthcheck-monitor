//! Human-readable facts are projected from allowlisted observation variants.
use crate::view::Fact;
use monitor_core::model::Data;
pub fn facts(data: &Data) -> Vec<Fact> {
    let mut facts = Vec::new();
    let mut add = |label: &str, value: String| {
        facts.push(Fact {
            label: label.into(),
            value,
        })
    };
    match data {
        Data::Workload {
            desired,
            ready,
            draining,
            ..
        } => {
            add("Ready replicas", format!("{ready} / {desired}"));
            if *draining {
                add("Lifecycle", "Draining".into());
            }
        }
        Data::Pod {
            ready,
            restarts,
            crash_loop,
            container,
            uid,
            created_at,
            terminated_at,
        } => {
            add("Container", container.clone());
            add("Pod UID", uid.clone());
            if let Some(at) = created_at {
                add("Created", at.to_rfc3339());
            }
            if let Some(at) = terminated_at {
                add("Last termination", at.to_rfc3339());
            }
            add("Ready", ready.to_string());
            add("Restarts", restarts.to_string());
            if *crash_loop {
                add("Runtime", "Crash loop".into());
            }
        }
        Data::Endpoint {
            dns,
            tls,
            status,
            latency_ms,
            expires_at,
            accepted,
        } => {
            add("DNS", if *dns { "Resolved" } else { "Unavailable" }.into());
            add("TLS", if *tls { "Trusted" } else { "Unavailable" }.into());
            add(
                "HTTP status",
                status.map_or("Unavailable".into(), |status| status.to_string()),
            );
            add(
                "Accepted HTTP statuses",
                if accepted.is_empty() {
                    "Below 500 (reachability only)".into()
                } else {
                    accepted
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            );
            add("Latency", format!("{latency_ms} ms"));
            if let Some(at) = expires_at {
                add("Certificate expires", at.to_rfc3339());
            }
        }
        Data::Queue {
            backlog,
            ready,
            desired,
            age_seconds,
            dead_letters,
            ..
        } => {
            add("Backlog", format!("{backlog:.0}"));
            add("Ready workers", format!("{ready} / {desired}"));
            if let Some(age) = age_seconds {
                add("Oldest age", format!("{age:.0} s"));
            }
            if let Some(count) = dead_letters {
                add("Dead letters", format!("{count:.0}"));
            }
        }
        Data::Metric {
            name,
            value,
            capacity,
            warning,
            error,
            window_seconds,
        } => {
            add(name, format!("{value:.2}"));
            if let Some(warning) = warning {
                add("Warning threshold", warning.to_string());
            }
            if let Some(error) = error {
                add("Error threshold", error.to_string());
            }
            add("Evaluation window", format!("{window_seconds} seconds"));
            if let Some(capacity) = capacity {
                add("Capacity", format!("{capacity:.2}"));
            }
        }
        Data::Service {
            state,
            replicas,
            backup_enabled,
            encrypted,
        } => {
            add("Service state", format!("{state:?}"));
            if let Some(count) = replicas {
                add("Replicas", count.to_string());
            }
            if let Some(backup) = backup_enabled {
                add("Backups", backup.to_string());
            }
            if let Some(encrypted) = encrypted {
                add("Encrypted", encrypted.to_string());
            }
        }
        Data::Build {
            pipeline,
            revision,
            state,
            ..
        } => {
            add("Pipeline", pipeline.clone());
            add("Revision", revision.clone());
            add("Build state", format!("{state:?}"));
        }
        Data::Image {
            desired,
            observed_digest,
            ..
        } => {
            add("Desired image", desired.clone());
            if let Some(digest) = observed_digest {
                add("Observed digest", digest.clone());
            }
        }
        Data::Condition { rule, healthy } => {
            add("Condition", rule.replace('-', " "));
            add(
                "Healthy",
                healthy.map_or("Unknown".into(), |value| value.to_string()),
            );
        }
        Data::Inventory { family, supported } => {
            add("Family", family.clone());
            add(
                "Coverage",
                if *supported {
                    "Supported metadata"
                } else {
                    "Inventory only"
                }
                .into(),
            );
        }
        Data::Identity { scope } => add("Identity", scope.clone()),
        _ => return crate::facts_metadata::facts(data),
    }
    facts
}
