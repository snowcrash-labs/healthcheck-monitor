//! Regional resources outside the selected locations remain inventory-only metadata.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{observation, text};
use serde_json::Value;
pub fn location(job: &Job, location: &str) -> bool {
    if job.target.regions.is_empty() {
        return true;
    }
    let location = location
        .rsplit('/')
        .next()
        .unwrap_or(location)
        .to_ascii_lowercase()
        .replace(' ', "");
    if matches!(
        location.as_str(),
        "global" | "us" | "eu" | "asia" | "nam4" | "eur4" | "asia1"
    ) {
        return true;
    }
    job.target.regions.iter().any(|region| {
        location == *region
            || location.strip_prefix(region).is_some_and(|suffix| {
                suffix.len() == 2
                    && suffix.starts_with('-')
                    && suffix.as_bytes()[1].is_ascii_lowercase()
            })
    })
}
pub fn selected(job: &Job, endpoint: &Endpoint, value: &Value) -> bool {
    if !matches!(job.target.provider, Provider::Gcp | Provider::Azure)
        || matches!(endpoint.id.split('/').next(), Some("resource-groups"))
    {
        return true;
    }
    let named_location = if job.target.provider == Provider::Gcp {
        text(value, &["/name"]).and_then(|name| {
            ["/locations/", "/zones/", "/regions/"]
                .into_iter()
                .find_map(|marker| {
                    name.split_once(marker)
                        .and_then(|(_, tail)| tail.split('/').next())
                })
        })
    } else {
        None
    };
    text(value, &["/location", "/region", "/zone"])
        .or(named_location)
        .is_none_or(|region| location(job, region))
}
pub fn inventory_only(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let name = text(value, &["/id", "/name"]).unwrap_or("regional-resource");
    vec![observation(
        job,
        &endpoint.id,
        name,
        Data::Inventory {
            family: "inventory-only-region".into(),
            supported: false,
        },
    )]
    .into_iter()
    .filter(|observation| {
        job.target.resources.is_empty()
            || job
                .target
                .resources
                .iter()
                .any(|selector| observation.resource.contains(selector))
    })
    .collect()
}
