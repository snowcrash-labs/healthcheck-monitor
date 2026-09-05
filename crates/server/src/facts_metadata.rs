//! Continuity, provenance, and diagnostic metadata rendered as bounded facts.
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
        Data::Certificate { issued, expires_at } => {
            add("Issued", known(*issued));
            if let Some(at) = expires_at {
                add("Expires", at.to_rfc3339());
            }
        }
        Data::Synchronization {
            ready,
            last_sync,
            interval_seconds,
        } => {
            add("Synchronized", known(*ready));
            if let Some(at) = last_sync {
                add("Last synchronized", at.to_rfc3339());
            }
            if let Some(seconds) = interval_seconds {
                add("Refresh interval", format!("{seconds} s"));
            }
        }
        Data::Recovery {
            state,
            enabled,
            last_attempt,
            last_success,
            retention_days,
            point_in_time,
            geo_redundant,
        } => {
            add("Recovery state", format!("{state:?}"));
            add("Enabled", known(*enabled));
            if let Some(at) = last_attempt {
                add("Last attempt", at.to_rfc3339());
            }
            if let Some(at) = last_success {
                add("Last success", at.to_rfc3339());
            }
            if let Some(days) = retention_days {
                add("Retention", format!("{days} days"));
            }
            add("Point-in-time recovery", known(*point_in_time));
            add("Geo redundant", known(*geo_redundant));
        }
        Data::Job {
            complete,
            failed,
            failed_attempts,
            succeeded,
            active,
            ..
        } => {
            add(
                "Job state",
                if *complete && *failed {
                    "Invalid terminal conditions"
                } else if *complete {
                    "Completed"
                } else if *failed {
                    "Failed"
                } else {
                    "In progress"
                }
                .into(),
            );
            add("Successful completions", succeeded.to_string());
            add("Failed attempts", failed_attempts.to_string());
            add("Active attempts", active.to_string());
        }
        Data::Schedule {
            schedule,
            timezone,
            suspended,
            last_schedule,
            last_success,
            ..
        } => {
            add("Schedule", schedule.clone());
            add("Timezone", timezone.clone());
            add("Suspended", suspended.to_string());
            if let Some(at) = last_schedule {
                add("Last scheduled", at.to_rfc3339());
            }
            if let Some(at) = last_success {
                add("Last success", at.to_rfc3339());
            }
        }
        Data::Log {
            signature,
            count,
            first_seen,
            last_seen,
            sampled,
        } => {
            add("Diagnostic", format!("{signature:?}"));
            add("Matching entries", count.to_string());
            add("First seen", first_seen.to_rfc3339());
            add("Last seen", last_seen.to_rfc3339());
            add("Sampled", sampled.to_string());
        }
        Data::LogWindow {
            start,
            end,
            scanned,
            duplicates,
            limit,
            complete,
            gap_seconds,
        } => {
            add("Window start", start.to_rfc3339());
            add("Window end", end.to_rfc3339());
            add("Sample count", format!("{scanned} / {limit}"));
            add("Duplicates", duplicates.to_string());
            add("Complete window", complete.to_string());
            add("Missing window", format!("{gap_seconds} s"));
        }
        Data::Commit {
            repository,
            revision,
            reference,
        } => {
            add("Repository", repository.clone());
            add("Commit", revision.clone());
            if let Some(reference) = reference {
                add("Reference", reference.clone());
            }
        }
        Data::Artifact {
            image,
            digest,
            tags,
            built,
            ..
        } => {
            add("Image", image.clone());
            add("Digest", digest.clone());
            add("Tags", tags.join(", "));
            add("Build confirmed", built.to_string());
        }
        Data::Provenance {
            registry_verified,
            build_verified,
            commit_verified,
            pending,
            mismatch,
            revision,
            ..
        } => {
            add("Registry verified", registry_verified.to_string());
            add("Build verified", build_verified.to_string());
            add("Commit verified", commit_verified.to_string());
            add("Deployment pending", pending.to_string());
            add("Revision mismatch", mismatch.to_string());
            if let Some(revision) = revision {
                add("Revision", revision.clone());
            }
        }
        Data::Slo {
            goal,
            compliance,
            budget,
            burn_rate,
            ..
        } => {
            add("Goal", format!("{:.3}%", goal * 100.0));
            if let Some(compliance) = compliance {
                add("Compliance", format!("{:.3}%", compliance * 100.0));
            }
            if let Some(budget) = budget {
                add("Budget", format!("{budget:.3}"));
            }
            if let Some(rate) = burn_rate {
                add("Burn rate", format!("{rate:.3}"));
            }
        }
        Data::SloDefinition { name, goal, .. } => {
            add("Objective", name.clone());
            if let Some(goal) = goal {
                add("Goal", format!("{:.3}%", goal * 100.0));
            }
        }
        Data::KeyMetadata {
            enabled,
            purpose,
            rotates_at,
            expires_at,
        } => {
            add("Enabled", known(*enabled));
            if let Some(purpose) = purpose {
                add("Purpose", purpose.clone());
            }
            if let Some(at) = rotates_at {
                add("Next rotation", at.to_rfc3339());
            }
            if let Some(at) = expires_at {
                add("Expires", at.to_rfc3339());
            }
        }
        Data::Maintenance { window, pending } => {
            add("Pending maintenance", pending.to_string());
            if let Some(window) = window {
                add("Window", window.clone());
            }
        }
        Data::NetworkPolicy {
            direction,
            allows_public,
            protocols,
            ports,
            priority,
        } => {
            if let Some(direction) = direction {
                add("Direction", direction.clone());
            }
            add("Public access", known(*allows_public));
            add("Protocols", protocols.join(", "));
            add("Ports", ports.join(", "));
            if let Some(priority) = priority {
                add("Priority", priority.to_string());
            }
        }
        Data::Quota {
            code,
            region,
            limit,
            ..
        } => {
            add("Quota", code.clone());
            add("Region", region.clone());
            add("Limit", format!("{limit:.2}"));
        }
        Data::Progress { state } => add("Progress", format!("{state:?}")),
        Data::Activity {
            operation,
            state,
            event_at,
        } => {
            add("Operation", operation.clone());
            add("State", format!("{state:?}"));
            if let Some(at) = event_at {
                add("Event time", at.to_rfc3339());
            }
        }
        Data::Registry { host } => add("Registry", host.clone()),
        Data::LogWorkspace {
            workspace_id,
            resource_id,
        } => {
            add("Workspace", workspace_id.clone());
            add("Resource", resource_id.clone());
        }
        Data::MetricResource {
            resource_id,
            namespace,
        } => {
            add("Resource", resource_id.clone());
            add("Metric namespace", namespace.clone());
        }
        Data::AdvertisedEndpoint { url } => add("Advertised endpoint", url.clone()),
        Data::Owner { .. } | Data::Scaler { .. } => {}
        _ => {}
    }
    facts
}
fn known(value: Option<bool>) -> String {
    value.map_or("Unknown".into(), |value| {
        if value { "Yes" } else { "No" }.into()
    })
}
