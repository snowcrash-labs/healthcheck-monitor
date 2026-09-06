//! Grouped evidence retains source timestamps and triggering facts.
use crate::view::{Fact, Resource};
use chrono::{DateTime, Utc};
use monitor_core::{
    diagnostics::{Diagnostic, ResourceContext},
    model::{Check, Data, Observation},
};
use serde::Serialize;
#[derive(Clone, Serialize)]
pub struct Evidence {
    pub id: String,
    pub check: Check,
    pub operation: String,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub context: Option<ResourceContext>,
    pub facts: Vec<Fact>,
}
impl crate::pages::Keyed for Evidence {
    fn key(&self) -> (u8, &str, &str) {
        (0, &self.id, "")
    }
}
impl Evidence {
    pub fn new(observation: &Observation, check: Check, expires_at: DateTime<Utc>) -> Self {
        let component = match &observation.data {
            Data::Pod { uid, container, .. } => format!("{uid}/{container}"),
            _ => String::new(),
        };
        Self {
            id: format!(
                "{check:?}/{}/{:?}/{component}",
                observation.operation,
                std::mem::discriminant(&observation.data)
            ),
            check,
            operation: observation.operation.clone(),
            observed_at: observation.observed_at,
            expires_at,
            context: observation.context.clone(),
            facts: crate::facts::facts(&observation.data),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct DiagnosticView {
    pub first_detected_at: Option<DateTime<Utc>>,
    pub last_detected_at: DateTime<Utc>,
    pub context: Option<ResourceContext>,
    pub facts: Vec<Fact>,
    pub links: Vec<crate::console_links::Link>,
}
impl From<&Diagnostic> for DiagnosticView {
    fn from(value: &Diagnostic) -> Self {
        let mut facts = crate::facts::facts(&value.observation.data);
        if let Some(prior) = value.previous_restarts {
            facts.push(Fact {
                label: "Previous restart count (same container UID)".into(),
                value: prior.to_string(),
            });
        }
        if matches!(
            value.observation.data,
            Data::Pod { .. } | Data::Workload { .. }
        ) {
            facts.push(Fact {
                label: "Startup grace".into(),
                value: format!("{} seconds", value.grace_seconds),
            });
        }
        let mut links = crate::console_links::links(value.observation.context.as_ref());
        links.extend(crate::log_links::links(
            value.observation.context.as_ref(),
            &value.observation.data,
        ));
        Self {
            first_detected_at: value.first_detected_at,
            last_detected_at: value.last_detected_at,
            context: value.observation.context.clone(),
            facts,
            links,
        }
    }
}
/// Keep separate check and component observations, even if a different source is newer.
pub fn attach(resource: &mut Resource, evidence: Evidence) {
    if !resource.checks.contains(&evidence.check) {
        resource.checks.push(evidence.check);
    }
    if resource.context.is_none() {
        resource.context = evidence.context.clone();
    }
    let rows = std::sync::Arc::make_mut(&mut resource.evidence);
    if let Some(old) = rows.iter_mut().find(|old| old.id == evidence.id) {
        if old.observed_at < evidence.observed_at {
            *old = evidence;
        }
    } else {
        rows.push(evidence);
    }
}
