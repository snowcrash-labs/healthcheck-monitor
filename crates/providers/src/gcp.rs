//! GCP native credentials and bounded metadata adapters, including regional builds.
use crate::{
    auth::Auth,
    common::{self, Endpoint},
};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::transport::Http;
use tokio_util::sync::CancellationToken;

pub fn endpoints(job: &Job) -> Vec<Endpoint> {
    let project = &job.target.scope;
    if job.check == Check::Preflight {
        return vec![Endpoint::get(
            "project-scope",
            format!("https://cloudresourcemanager.googleapis.com/v3/projects/{project}"),
            "",
        )];
    }
    if job.check == Check::Alerts {
        return vec![
            Endpoint::get(
                "open-alerts",
                format!(
                    "https://monitoring.googleapis.com/v3/projects/{project}/alerts?filter=state%3DOPEN"
                ),
                "/alerts",
            ),
            Endpoint::get(
                "snoozes",
                format!("https://monitoring.googleapis.com/v3/projects/{project}/snoozes"),
                "/snoozes",
            ),
        ];
    }
    if job.check == Check::Slo {
        return vec![Endpoint::get(
            "slo-services",
            format!("https://monitoring.googleapis.com/v3/projects/{project}/services"),
            "/services",
        )];
    }
    let regions: Vec<_> = job
        .target
        .regions
        .iter()
        .map(String::as_str)
        .chain(std::iter::once("global"))
        .collect();
    let mut endpoints = Vec::new();
    for (id, template, items) in CATALOG {
        if job.check == Check::Edge && *id != "cloud-run" {
            continue;
        }
        if job.check == Check::Releases
            && !matches!(
                *id,
                "builds" | "build-triggers" | "artifact-repositories" | "cloud-run"
            )
        {
            continue;
        }
        if job.check == Check::Managed
            && !matches!(
                *id,
                "sql"
                    | "redis"
                    | "valkey"
                    | "buckets"
                    | "kms-keyrings"
                    | "secrets"
                    | "provider-health"
                    | "quotas"
            )
        {
            continue;
        }
        for region in &regions {
            if *region == "global"
                && template.contains("{r}")
                && !matches!(*id, "builds" | "build-triggers")
            {
                continue;
            }
            if !template.contains("{r}") && *region != regions[0] {
                continue;
            }
            endpoints.push(Endpoint::get(
                format!("{id}/{region}"),
                format!(
                    "https://{}",
                    template.replace("{p}", project).replace("{r}", region)
                ),
                items,
            ));
        }
    }
    for secret in &job.target.watched_secrets {
        endpoints.push(Endpoint::get(format!("secret-versions/{secret}"), format!("https://secretmanager.googleapis.com/v1/projects/{project}/secrets/{secret}/versions"), "/versions"));
    }
    endpoints
}
pub async fn collect(
    http: &Http,
    auth: &Auth,
    job: &Job,
    cancel: &CancellationToken,
    cache: &crate::inventory_cache::InventoryCache,
) -> CheckResult {
    if job.check == Check::Slo {
        return crate::gcp_slo::collect_from(
            &common::NativeSource {
                http,
                auth,
                cache: Some(cache),
                dedupe: None,
            },
            job,
            cancel,
        )
        .await;
    }
    if job.check == Check::Metrics || job.check == Check::Queues {
        let mut job = job.clone();
        if job.target.metrics.is_empty() {
            job.target.metrics = crate::metric_catalog::gcp();
        }
        return crate::metrics::gcp(http, auth, &job, cancel).await;
    }
    if job.check == Check::Logs {
        return crate::gcp_logs::collect_from(
            &common::NativeSource {
                dedupe: None,
                http,
                auth,
                cache: None,
            },
            job,
            cancel,
        )
        .await;
    }
    common::collect_cached(http, auth, job, endpoints(job), cancel, cache).await
}
use crate::gcp_catalog::CATALOG;
