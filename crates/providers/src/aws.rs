//! AWS account identity, native CloudWatch batches, and signed read-only APIs.
use crate::{
    auth::Auth,
    common::{self, Endpoint},
};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{observation, operation},
    transport::{Error, Http},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

pub fn endpoints(job: &Job) -> Vec<Endpoint> {
    let mut out = Vec::new();
    if matches!(job.check, Check::Inventory | Check::Managed | Check::Edge) {
        for (id, service, path, items) in [
            (
                "route53",
                "route53",
                "/2013-04-01/hostedzone",
                "/HostedZones/HostedZone",
            ),
            (
                "cloudfront",
                "cloudfront",
                "/2020-05-31/distribution",
                "/Items/DistributionSummary",
            ),
        ] {
            let mut endpoint =
                Endpoint::get(id, format!("https://{service}.amazonaws.com{path}"), items);
            endpoint.aws = Some((service.into(), "us-east-1".into(), "".into()));
            out.push(endpoint);
        }
    }
    for region in &job.target.regions {
        for (name, service, prefix, action, items) in JSON_APIS {
            if job.check == Check::Edge {
                continue;
            }
            if job.check == Check::Alerts && *name != "health" {
                continue;
            }
            if job.check == Check::Releases
                && !matches!(*name, "ecr" | "codebuild" | "codepipeline" | "ecs-services")
            {
                continue;
            }
            let mut endpoint = Endpoint::get(
                format!("{name}/{region}"),
                format!("https://{service}.{region}.amazonaws.com/"),
                items,
            );
            endpoint.aws = Some((
                (*service).into(),
                region.clone(),
                format!("{prefix}.{action}"),
            ));
            endpoint.body = Some(match *name {
                "quotas" => json!({"ServiceCode":"ec2","MaxResults":100}),
                "health" => {
                    json!({"filter":{"eventStatusCodes":["open","upcoming"]},"maxResults":100})
                }
                "dynamodb" => json!({"Limit":100}),
                _ => json!({}),
            });
            out.push(endpoint);
        }
        for (name, service, action, version, items) in QUERY_APIS {
            if job.check == Check::Edge && *name != "load-balancers" {
                continue;
            }
            if matches!(job.check, Check::Alerts | Check::Releases | Check::Logs) {
                continue;
            }
            let mut endpoint = Endpoint::get(
                format!("{name}/{region}"),
                format!("https://{service}.{region}.amazonaws.com/"),
                items,
            );
            endpoint.aws = Some(((*service).into(), region.clone(), format!("query:{action}")));
            endpoint.body = Some(json!({"Action":action,"Version":version}));
            out.push(endpoint);
        }
        for (name, service, path, items) in [
            ("eks", "eks", "/clusters", "/clusters"),
            ("lambda", "lambda", "/2015-03-31/functions/", "/Functions"),
            ("s3", "s3", "/", "/Buckets/Bucket"),
            ("backup", "backup", "/backup-vaults/", "/BackupVaultList"),
            ("acm", "acm", "/", "/CertificateSummaryList"),
        ] {
            if name == "acm" {
                continue;
            }
            let mut endpoint = Endpoint::get(
                format!("{name}/{region}"),
                format!("https://{service}.{region}.amazonaws.com{path}"),
                items,
            );
            endpoint.aws = Some((service.into(), region.clone(), "".into()));
            out.push(endpoint);
        }
    }
    if job.check == Check::Discovery {
        let mut endpoint = Endpoint::get(
            "organization-accounts",
            "https://organizations.us-east-1.amazonaws.com/",
            "/Accounts",
        );
        endpoint.body = Some(json!({"MaxResults":20}));
        endpoint.aws = Some((
            "organizations".into(),
            "us-east-1".into(),
            "AWSOrganizationsV20161128.ListAccounts".into(),
        ));
        out.push(endpoint);
    }
    out
}
pub async fn collect(
    http: &Http,
    auth: &Auth,
    job: &Job,
    cancel: &CancellationToken,
    cache: &crate::inventory_cache::InventoryCache,
) -> CheckResult {
    if job.check == Check::Logs {
        return crate::cloud_logs::aws(http, auth, job, cancel).await;
    }
    if job.check == Check::Preflight {
        return identity(auth, job).await;
    }
    if job.check == Check::Metrics || job.check == Check::Queues {
        return crate::metrics::aws(auth, job, cancel).await;
    }
    let mut result = common::collect_cached(http, auth, job, endpoints(job), cancel, cache).await;
    crate::quotas::evaluate(auth, job, &mut result, cancel).await;
    result
}
async fn identity(auth: &Auth, job: &Job) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Unauthenticated,
    );
    let Auth::Aws(clients) = auth else {
        return result;
    };
    let outcome = clients.sts.get_caller_identity().send().await;
    match outcome {
        Ok(identity) if identity.account() == Some(job.target.scope.as_str()) => {
            result.operations = vec![operation("caller-identity", Ok(1), 1, true)];
            result.observations.push(observation(
                job,
                "caller-identity",
                "current",
                Data::Identity {
                    scope: identity
                        .arn()
                        .map(monitor_integrations::projection::identity)
                        .unwrap_or_default(),
                },
            ));
        }
        Ok(_) => {
            result.operations = vec![operation(
                "caller-identity",
                Err(&Error::Forbidden),
                1,
                true,
            )]
        }
        Err(_) => {}
    }
    result
}
use crate::aws_catalog::{JSON_APIS, QUERY_APIS};
