//! Journal records contain monitor-owned diagnostic metadata and stable retry keys.
use crate::{
    enums,
    error::Error,
    types::{Digest, Evidence, Name, Resource},
};
use chrono::{DateTime, Utc};
use monitor_core::model::{CheckResult, Finding, Transition};
use serde::Serialize;
use sha2::Digest as _;

pub fn digest(value: &impl Serialize) -> Result<Digest, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Record)?;
    let mut encoded = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in sha2::Sha256::digest(bytes) {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 15)] as char);
    }
    Digest::try_new(encoded).map_err(|_| Error::Record)
}
#[derive(Clone, Serialize)]
pub struct Run {
    pub(crate) key: Digest,
    pub(crate) target: Name,
    pub(crate) check: enums::Check,
    pub(crate) started_at: DateTime<Utc>,
    pub(crate) finished_at: DateTime<Utc>,
    pub(crate) complete: bool,
    pub(crate) observations: i64,
    pub(crate) failures: i64,
}
impl Run {
    pub fn new(result: &CheckResult) -> Result<Self, Error> {
        // Cache reuse preserves the original data timestamps even when invocation starts later.
        let finished_at = result.finished_at.max(result.started_at);
        Ok(Self {
            key: digest(&(
                &result.revision,
                &result.target,
                result.check,
                result.started_at,
                result.finished_at,
            ))?,
            target: Name::try_new(result.target.clone()).map_err(|_| Error::Record)?,
            check: result.check.into(),
            started_at: result.started_at,
            finished_at,
            complete: result.complete(),
            observations: i64::try_from(result.observations.len()).map_err(|_| Error::Record)?,
            failures: i64::try_from(
                result
                    .operations
                    .iter()
                    .filter(|operation| {
                        operation.required
                            && operation.coverage != monitor_core::model::Coverage::Complete
                    })
                    .count(),
            )
            .map_err(|_| Error::Record)?,
        })
    }
}
#[derive(Clone, Serialize)]
pub struct Event {
    pub(crate) key: Digest,
    pub(crate) target: Name,
    pub(crate) finding: Resource,
    pub(crate) resource: Resource,
    pub(crate) rule: Name,
    pub(crate) kind: enums::Kind,
    pub(crate) severity: enums::Severity,
    pub(crate) expected: enums::Expected,
    pub(crate) confidence: enums::Confidence,
    pub(crate) observed_at: DateTime<Utc>,
    pub(crate) at: DateTime<Utc>,
    pub(crate) stale: bool,
    pub(crate) evidence: Vec<Evidence>,
}
impl Event {
    pub fn new(target: &str, transition: &Transition, finding: &Finding) -> Result<Self, Error> {
        if finding.evidence.len() > 64 || transition.finding != finding.id {
            return Err(Error::Record);
        }
        Ok(Self {
            key: digest(&(transition.at, &transition.finding, transition.kind))?,
            target: Name::try_new(target.to_owned()).map_err(|_| Error::Record)?,
            finding: Resource::try_new(finding.id.clone()).map_err(|_| Error::Record)?,
            resource: Resource::try_new(finding.resource.clone()).map_err(|_| Error::Record)?,
            rule: Name::try_new(finding.rule.clone()).map_err(|_| Error::Record)?,
            kind: transition.kind.into(),
            severity: finding.severity.into(),
            expected: finding.expected.into(),
            confidence: finding.confidence.into(),
            observed_at: finding.observed_at,
            at: transition.at,
            stale: finding.stale,
            evidence: finding
                .evidence
                .iter()
                .map(|value| Evidence::try_new(value.clone()).map_err(|_| Error::Record))
                .collect::<Result<_, _>>()?,
        })
    }
}
#[derive(Clone, Serialize)]
pub struct Gap {
    pub(crate) key: Digest,
    pub(crate) at: DateTime<Utc>,
    pub(crate) events: i64,
    pub(crate) runs: i64,
}
impl Gap {
    pub fn new(at: DateTime<Utc>, events: u64, runs: u64) -> Result<Self, Error> {
        Ok(Self {
            key: digest(&(at, events, runs))?,
            at,
            events: i64::try_from(events).map_err(|_| Error::Record)?,
            runs: i64::try_from(runs).map_err(|_| Error::Record)?,
        })
    }
}
