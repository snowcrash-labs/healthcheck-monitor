//! Key Vault list and version operations expose metadata only, never secret values.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::projection::{boolean, observation, text, timestamp};
use serde_json::Value;
pub fn followups(job: &Job, parent: &Endpoint, value: &Value) -> Vec<Endpoint> {
    let family = parent.id.split('/').next().unwrap_or("");
    if family == "key-vaults" {
        let Some(base) = text(value, &["/properties/vaultUri"])
            .and_then(|uri| url::Url::parse(uri).ok())
            .filter(valid_url)
        else {
            return vec![];
        };
        return ["keys", "secrets", "certificates"]
            .into_iter()
            .filter_map(|kind| {
                base.join(&format!(
                    "{kind}?api-version=2025-07-01&maxresults={}",
                    job.settings.page_size.min(25)
                ))
                .ok()
                .map(|url| {
                    Endpoint::get(
                        format!("kv-{kind}/{}", base.host_str().unwrap_or("vault")),
                        url.as_str(),
                        "/value",
                    )
                })
            })
            .collect();
    }
    if !matches!(family, "kv-keys" | "kv-secrets") {
        return vec![];
    }
    let Some(id) = text(value, &["/id", "/kid"])
        .and_then(|id| url::Url::parse(id).ok())
        .filter(valid_url)
    else {
        return vec![];
    };
    let Ok(base) = url::Url::parse(&parent.url) else {
        return vec![];
    };
    if id.origin() != base.origin() {
        return vec![];
    }
    let parts: Vec<_> = id.path().trim_matches('/').split('/').take(2).collect();
    if parts.len() != 2 || !matches!(parts[0], "keys" | "secrets") {
        return vec![];
    }
    let mut url = id.clone();
    url.set_path(&format!("/{}/{}/versions", parts[0], parts[1]));
    url.set_query(Some("api-version=2025-07-01"));
    vec![Endpoint::get(
        format!("kv-versions/{id}"),
        url.as_str(),
        "/value",
    )]
}
fn valid_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.ends_with(".vault.azure.net"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}
pub fn project(job: &Job, endpoint: &Endpoint, value: &Value) -> Vec<Observation> {
    let name = text(value, &["/id", "/kid"]).unwrap_or("metadata");
    if endpoint.id.starts_with("kv-certificates/") {
        return vec![observation(
            job,
            &endpoint.id,
            name,
            Data::Certificate {
                issued: boolean(value, &["/attributes/enabled"]),
                expires_at: timestamp(value, &["/attributes/exp"]),
            },
        )];
    }
    vec![observation(
        job,
        &endpoint.id,
        name,
        Data::KeyMetadata {
            enabled: boolean(value, &["/attributes/enabled"]),
            purpose: None,
            rotates_at: None,
            expires_at: timestamp(value, &["/attributes/exp"]),
        },
    )]
}
