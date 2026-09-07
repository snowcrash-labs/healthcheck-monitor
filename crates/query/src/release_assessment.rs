//! Release verification folds paginated evidence into a fixed-size result.
use crate::{
    filter::{Deployment, Window},
    record::{Details, Record},
};
use chrono::{DateTime, Utc};
#[derive(Default)]
pub struct ReleaseAssessment {
    pub fresh: u64,
    pub mismatch: bool,
    pub pending: bool,
    pub missing_digest: bool,
    pub unverified_revision: bool,
}
impl ReleaseAssessment {
    pub fn observe(
        &mut self,
        query: &Deployment,
        record: &Record,
        window: Window,
        now: DateTime<Utc>,
    ) {
        if record.last_observed_at < window.from
            || record.observed_at >= window.to
            || record.stale
            || record.valid_until.is_none_or(|at| at < window.to.min(now))
        {
            return;
        }
        let Details::Release {
            revision,
            observed_digests,
            pending,
            verified,
            ..
        } = &record.details
        else {
            return;
        };
        self.fresh = self.fresh.saturating_add(1);
        self.pending |= *pending;
        if let Some(expected) = &query.expected_digest {
            self.missing_digest |= observed_digests.is_empty();
            self.mismatch |= observed_digests.iter().any(|d| d != expected);
        }
        if let Some(expected) = &query.expected_revision {
            self.unverified_revision |= !*verified || revision.is_none();
            self.mismatch |= *verified && revision.as_ref().is_some_and(|r| r != expected);
        }
    }
    pub fn reasons(&self) -> Vec<String> {
        let mut reasons = vec![];
        for (condition, text) in [
            (
                self.fresh == 0,
                "Expected release has not been observed after deployment",
            ),
            (self.pending, "Release rollout is still pending"),
            (self.missing_digest, "Observed image digest is unavailable"),
            (
                self.unverified_revision,
                "Observed source revision is not verified",
            ),
        ] {
            if condition {
                reasons.push(text.into());
            }
        }
        reasons
    }
}
