//! Explicit time windows and scope filters; relative windows are frozen into pagination cursors.
use crate::{Error, enums::*};
use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Filter {
    pub target: Option<String>,
    pub provider: Option<Provider>,
    /// Native provider scope: GCP project, AWS account, or Azure subscription.
    pub scope: Option<String>,
    pub project: Option<String>,
    pub account: Option<String>,
    pub subscription: Option<String>,
    pub region: Option<String>,
    pub cluster: Option<String>,
    pub namespace: Option<String>,
    pub service: Option<String>,
    /// Monitoring category, distinct from the DNS hostname filter.
    pub check: Option<Check>,
    pub resource: Option<String>,
    pub hostname: Option<String>,
    pub severity: Option<Severity>,
    pub state: Option<FindingState>,
    pub q: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// Relative window in seconds; defaults to 3600. Cannot accompany from or to.
    pub lookback_seconds: Option<u32>,
    pub cursor: Option<String>,
    /// Page size, from 1 to 100; defaults to 50.
    pub limit: Option<u16>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}
impl Filter {
    /// Normalize provider aliases before both execution and cursor binding.
    pub fn normalize(&mut self) -> Result<(), Error> {
        let aliases = [
            (self.project.take(), Provider::Gcp),
            (self.account.take(), Provider::Aws),
            (self.subscription.take(), Provider::Azure),
        ];
        for (scope, provider) in aliases {
            if let Some(scope) = scope {
                if self.scope.as_ref().is_some_and(|old| old != &scope)
                    || self.provider.is_some_and(|old| old != provider)
                {
                    return Err(Error::Filter);
                }
                self.scope = Some(scope);
                self.provider = Some(provider);
            }
        }
        for value in [
            &self.target,
            &self.scope,
            &self.region,
            &self.cluster,
            &self.namespace,
            &self.service,
            &self.resource,
            &self.hostname,
            &self.q,
        ]
        .into_iter()
        .flatten()
        {
            if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
                return Err(Error::Filter);
            }
        }
        if let Some(host) = &mut self.hostname {
            *host = host.trim_end_matches('.').to_ascii_lowercase();
            if host.is_empty()
                || host.len() > 253
                || !host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            {
                return Err(Error::Filter);
            }
        }
        if !(1..=100).contains(&self.limit.unwrap_or(50))
            || self.q.as_ref().is_some_and(|s| s.len() > 128)
            || self.cursor.as_ref().is_some_and(|s| s.len() > 2048)
        {
            return Err(Error::Filter);
        }
        Ok(())
    }
    pub fn window(&self, now: DateTime<Utc>) -> Result<Window, Error> {
        if self.lookback_seconds.is_some() && (self.from.is_some() || self.to.is_some()) {
            return Err(Error::Filter);
        }
        let to = self.to.unwrap_or(now);
        let from = match self.from {
            Some(from) => from,
            None => to
                .checked_sub_signed(Duration::seconds(i64::from(
                    self.lookback_seconds.unwrap_or(3600),
                )))
                .ok_or(Error::Filter)?,
        };
        if to.checked_sub_signed(Duration::days(31)).is_none() {
            return Err(Error::Filter);
        }
        if from >= to || to > now + Duration::seconds(30) || to - from > Duration::days(31) {
            return Err(Error::Filter);
        }
        Ok(Window { from, to })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub deployed_at: DateTime<Utc>,
    /// Assessment duration, 60 through 86400 seconds; defaults to 300.
    pub window_seconds: Option<u32>,
    pub expected_revision: Option<String>,
    pub expected_digest: Option<String>,
    #[serde(flatten)]
    pub filter: Filter,
}
impl Deployment {
    pub fn window(&self) -> Result<Window, Error> {
        let seconds = self.window_seconds.unwrap_or(300);
        if !(60..=86400).contains(&seconds)
            || self.filter.severity.is_some()
            || self.filter.state.is_some()
            || self.filter.q.is_some()
            || self.filter.from.is_some()
            || self.filter.to.is_some()
            || self.filter.lookback_seconds.is_some()
            || self.filter.cursor.is_some()
            || self.filter.target.is_none()
                && self.filter.resource.is_none()
                && self.filter.scope.is_none()
                && self.filter.project.is_none()
                && self.filter.account.is_none()
                && self.filter.subscription.is_none()
        {
            return Err(Error::Filter);
        }
        for value in [&self.expected_revision, &self.expected_digest]
            .into_iter()
            .flatten()
        {
            if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                return Err(Error::Filter);
            }
        }
        let duration = Duration::seconds(i64::from(seconds));
        self.deployed_at
            .checked_sub_signed(duration)
            .ok_or(Error::Filter)?;
        let to = self
            .deployed_at
            .checked_add_signed(duration)
            .ok_or(Error::Filter)?;
        Ok(Window {
            from: self.deployed_at,
            to,
        })
    }
}
