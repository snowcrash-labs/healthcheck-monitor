//! Native resource identity and compact triggering evidence survive collection failures.
use crate::model::{Observation, Provider};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Only metadata required to identify and locate an infrastructure resource is retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceContext {
    pub provider: Provider,
    pub scope: String,
    pub service: String,
    pub native_id: String,
    pub region: Option<String>,
    pub zone: Option<String>,
    pub cluster: Option<String>,
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub uid: Option<String>,
    pub container: Option<String>,
}

/// Detection times belong to a finding episode, not to later recovery evaluations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub first_detected_at: Option<DateTime<Utc>>,
    pub last_detected_at: DateTime<Utc>,
    pub observation: Observation,
    pub previous_restarts: Option<u32>,
    pub grace_seconds: u64,
}

impl Diagnostic {
    /// A restored legacy finding has an unknown beginning until a new episode starts.
    pub fn capture(
        observation: &Observation,
        previous: Option<&Observation>,
        old: Option<&crate::model::Finding>,
        grace_seconds: u64,
    ) -> Self {
        let previous_restarts = match (previous.map(|obs| &obs.data), &observation.data) {
            (
                Some(crate::model::Data::Pod {
                    uid: prior_uid,
                    container: prior_container,
                    restarts,
                    ..
                }),
                crate::model::Data::Pod { uid, container, .. },
            ) if prior_uid == uid && prior_container == container => Some(*restarts),
            _ => None,
        };
        Self {
            first_detected_at: match old {
                Some(old) => old
                    .diagnostic
                    .as_ref()
                    .and_then(|diagnostic| diagnostic.first_detected_at),
                None => Some(observation.observed_at),
            },
            last_detected_at: observation.observed_at,
            observation: observation.clone(),
            previous_restarts,
            grace_seconds,
        }
    }
}
