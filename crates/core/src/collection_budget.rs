//! Per-collection admission bounds evidence before pages and provider results accumulate.
use crate::{config::settings::Settings, model::*};
pub struct Limit {
    used: usize,
    bytes: usize,
    assets: usize,
    operations: usize,
    limited: bool,
}
impl Limit {
    pub fn new(settings: &Settings) -> Self {
        let total = settings.memory_bytes / 2 / settings.concurrency.max(1);
        Self {
            used: 0,
            bytes: total.saturating_sub((total / 4).min(4096)),
            assets: settings.max_assets,
            operations: 0,
            limited: false,
        }
    }
    pub fn exhausted(&self) -> bool {
        self.used >= self.bytes || self.operations >= self.assets
    }
    pub fn mark_limited(&mut self) {
        self.limited = true;
    }
    pub fn observations(
        &mut self,
        out: &mut Vec<Observation>,
        incoming: impl IntoIterator<Item = Observation>,
    ) -> bool {
        for observation in incoming {
            let size = crate::bounds::observation_bytes(&observation);
            if out.len() >= self.assets || size > self.bytes.saturating_sub(self.used) {
                self.limited = true;
                return false;
            }
            self.used += size;
            out.push(observation);
        }
        true
    }
    pub fn operations(
        &mut self,
        out: &mut Vec<Operation>,
        incoming: impl IntoIterator<Item = Operation>,
    ) -> bool {
        for operation in incoming {
            let size = crate::bounds::operation_bytes(&operation);
            if self.operations >= self.assets || size > self.bytes.saturating_sub(self.used) {
                self.limited = true;
                return false;
            }
            self.used += size;
            self.operations += 1;
            out.push(operation);
        }
        true
    }
    pub fn merge(&mut self, out: &mut CheckResult, incoming: CheckResult) {
        let start = out.operations.len();
        self.operations(&mut out.operations, incoming.operations);
        if !self.observations(&mut out.observations, incoming.observations) {
            for operation in &mut out.operations[start..] {
                if operation.coverage == Coverage::Complete {
                    operation.coverage = Coverage::Truncated;
                }
            }
        }
    }
    pub fn finish(&mut self, result: &mut CheckResult, required: bool) {
        if self.limited {
            result.operations.push(Operation {
                id: "collection-budget".into(),
                coverage: Coverage::Truncated,
                observed_at: chrono::Utc::now(),
                records: result.observations.len(),
                pages: 0,
                attempts: 0,
                required,
            });
        }
    }
}
