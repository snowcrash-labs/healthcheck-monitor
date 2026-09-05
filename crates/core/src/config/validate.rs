//! Reject unsafe identifiers, contradictory limits, and ambiguous selections.
use super::types::Config;
use crate::error::Error;
use std::collections::BTreeSet;
/// Identifiers cannot inject query syntax, path traversal, or terminal control codes.
pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.contains("..")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:@".contains(&b))
}
impl Config {
    pub fn validate(&self) -> Result<(), Error> {
        super::credentials::validate(self)?;
        if self.version != 1 || self.targets.is_empty() || self.targets.len() > 128 {
            return Err(Error::Config(
                "version must be 1 with 1..128 explicit targets".into(),
            ));
        }
        if self.discovery.len() > 128
            || self.credentials.len() > 128
            || self.profiles.len() > 64
            || self.severity.len() > 512
        {
            return Err(Error::Config(
                "configuration exceeds registry bounds".into(),
            ));
        }
        let mut names = BTreeSet::new();
        for target in &self.targets {
            if let Some(name) = &target.github_credential
                && !self
                    .credentials
                    .get(name)
                    .is_some_and(|credential| credential.provider == crate::model::Provider::Github)
            {
                return Err(Error::Config(
                    "GitHub credentials must reference a GitHub profile".into(),
                ));
            }
            if target.artifact_targets.len() > 128
                || target
                    .artifact_targets
                    .iter()
                    .any(|name| !self.targets.iter().any(|target| &target.name == name))
            {
                return Err(Error::Config(
                    "artifact targets must reference configured targets".into(),
                ));
            }
            if target.repositories.len() > 128
                || target.resources.len() > 1024
                || target.watched_secrets.len() > 512
                || target.repository_refs.len() > 128
                || target
                    .repositories
                    .iter()
                    .chain(target.repository_refs.keys())
                    .chain(target.build_repositories.values())
                    .any(|repo| repo.split('/').count() != 2 || repo.split('/').any(str::is_empty))
                || target
                    .repository_refs
                    .values()
                    .any(|reference| !identifier(reference))
            {
                return Err(Error::Config("repository references need owner/repository identities within configured bounds".into()));
            }
            if target.build_targets.len() > 512
                || target.build_repositories.len() > 512
                || target
                    .build_targets
                    .iter()
                    .chain(&target.build_repositories)
                    .any(|(key, value)| !identifier(key) || !identifier(value))
            {
                return Err(Error::Config(
                    "invalid build target or repository mapping".into(),
                ));
            }
            if target.slo_goals.len() > 128
                || target.slo_goals.iter().any(|(name, goal)| {
                    !goal.is_finite()
                        || *goal <= 0.0
                        || *goal > 1.0
                        || !target.metrics.iter().any(|metric| {
                            metric.name == *name
                                && matches!(metric.aggregation, super::types::Aggregation::Latest)
                        })
                })
            {
                return Err(Error::Config("SLO goals need a fraction in (0,1] and a named latest-aggregation compliance metric".into()));
            }
            if target
                .timezone
                .as_ref()
                .is_some_and(|zone| zone.parse::<chrono_tz::Tz>().is_err())
            {
                return Err(Error::Config("invalid controller timezone".into()));
            }
            if target.expectations.len() > 128
                || target.expectations.keys().any(|key| !identifier(key))
            {
                return Err(Error::Config("invalid resource expectations".into()));
            }
            if let Some(change) = &target.change
                && (!identifier(&change.repository)
                    || !identifier(&change.base)
                    || !identifier(&change.head)
                    || change.paths.is_empty()
                    || change.paths.len() > 128
                    || change.paths.iter().any(|path| !identifier(path))
                    || change.workloads.len() > 1024
                    || change
                        .workloads
                        .iter()
                        .any(|(path, workload)| !identifier(path) || !identifier(workload)))
            {
                return Err(Error::Config("invalid repository change scope".into()));
            }
            if target.flows.len() > 16 {
                return Err(Error::Config("at most 16 flows per target".into()));
            }
            for flow in &target.flows {
                if target.metrics.iter().any(|metric| {
                    (metric.name == flow.demand
                        || flow
                            .stages
                            .iter()
                            .any(|stage| stage.progress == metric.name))
                        && !matches!(metric.aggregation, super::types::Aggregation::Latest)
                }) {
                    return Err(Error::Config(
                        "flow demand and progress metrics must use latest aggregation".into(),
                    ));
                }
                if !identifier(&flow.name)
                    || !identifier(&flow.demand)
                    || flow.stages.is_empty()
                    || flow.stages.len() > 16
                    || flow.stages.iter().any(|stage| {
                        !identifier(&stage.name)
                            || !identifier(&stage.progress)
                            || stage.workload.as_ref().is_some_and(|w| !identifier(w))
                    })
                {
                    return Err(Error::Config("invalid flow or stage configuration".into()));
                }
            }
            if !identifier(&target.name)
                || target.name.contains(['/', ':', '@'])
                || !identifier(&target.scope)
                || !names.insert(&target.name)
            {
                return Err(Error::Config(
                    "target names must be unique and scope identifiers valid".into(),
                ));
            }
            if let Some(name) = &target.credential {
                let profile = self
                    .credentials
                    .get(name)
                    .ok_or_else(|| Error::Config("unknown credential profile".into()))?;
                if profile.provider != target.provider {
                    return Err(Error::Config(
                        "credential provider differs from target".into(),
                    ));
                }
            }
            for value in target
                .regions
                .iter()
                .chain(&target.resources)
                .chain(&target.repositories)
                .chain(&target.watched_secrets)
                .chain(target.context.iter())
            {
                if !identifier(value) {
                    return Err(Error::Config("invalid resource selector".into()));
                }
            }
            if target.regions.len() > 32
                || target.metrics.len() > 500
                || target.endpoints.len() > 256
            {
                return Err(Error::Config("target exceeds collection bounds".into()));
            }
            for endpoint in &target.endpoints {
                if endpoint.url.scheme() != "https"
                    || endpoint.url.host_str().is_none()
                    || !endpoint.url.username().is_empty()
                    || endpoint.url.password().is_some()
                    || endpoint.url.query().is_some()
                    || endpoint.url.fragment().is_some()
                    || endpoint.accepted.is_empty()
                    || endpoint.accepted.iter().any(|s| !(100..=599).contains(s))
                {
                    return Err(Error::Config("endpoint needs an HTTPS URL without credentials/query and explicit accepted statuses".into()));
                }
            }
            if let Some(url) = &target.nats_url
                && (!matches!(url.scheme(), "tls" | "nats")
                    || !url.username().is_empty()
                    || url.password().is_some())
            {
                return Err(Error::Config(
                    "NATS URL must not contain credentials".into(),
                ));
            }
            for metric in &target.metrics {
                if !identifier(&metric.name)
                    || !identifier(&metric.namespace)
                    || metric.metric.is_empty()
                    || metric.metric.len() > 256
                    || metric.metric.contains(',')
                    || metric.metric.chars().any(char::is_control)
                    || !identifier(&metric.resource)
                    || metric.dimensions.len() > 30
                    || metric.dimensions.iter().any(|(key, value)| {
                        !identifier(key) || value.len() > 512 || value.chars().any(char::is_control)
                    })
                {
                    return Err(Error::Config("invalid metric identity".into()));
                }
                if [metric.capacity, metric.warning, metric.error]
                    .into_iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v < 0.0)
                    || metric.capacity == Some(0.0)
                {
                    return Err(Error::Config("invalid metric threshold or capacity".into()));
                }
            }
        }
        for root in &self.discovery {
            if let Some(name) = &root.credential
                && !self
                    .credentials
                    .get(name)
                    .is_some_and(|credential| credential.provider == root.provider)
            {
                return Err(Error::Config(
                    "discovery credentials must match the root provider".into(),
                ));
            }
            if !identifier(&root.scope) {
                return Err(Error::Config("invalid discovery scope".into()));
            }
        }
        Ok(())
    }
}
