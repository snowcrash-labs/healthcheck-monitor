//! Fixed console origins and encoded native identities prevent arbitrary destination links.
use monitor_core::{diagnostics::ResourceContext, model::Provider};
use serde::Serialize;
#[derive(Clone, Serialize)]
pub struct Link {
    pub label: String,
    pub url: String,
}
pub(crate) fn encoded(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}
pub fn links(context: Option<&ResourceContext>) -> Vec<Link> {
    let Some(c) = context else {
        return vec![];
    };
    if [&c.native_id, &c.scope]
        .iter()
        .any(|s| s.is_empty() || s.len() > 2048 || s.chars().any(char::is_control))
    {
        return vec![];
    }
    let name = c
        .name
        .as_deref()
        .unwrap_or_else(|| c.native_id.rsplit('/').next().unwrap_or(&c.native_id));
    let (label, destination) = match c.provider {
        Provider::Azure
            if c.native_id
                .to_ascii_lowercase()
                .starts_with(&format!("/subscriptions/{}/", c.scope.to_ascii_lowercase())) =>
        {
            (
                "Open in Azure",
                format!(
                    "https://portal.azure.com/#resource{}/overview",
                    c.native_id
                        .split('/')
                        .map(encoded)
                        .collect::<Vec<_>>()
                        .join("/")
                ),
            )
        }
        Provider::Azure => (
            "Open Azure subscription",
            format!(
                "https://portal.azure.com/#resource/subscriptions/{}/overview",
                encoded(&c.scope)
            ),
        ),
        Provider::Gcp => {
            let project = encoded(&c.scope);
            let resource = encoded(name);
            let region = c.region.as_deref().map(encoded);
            let location = c.zone.as_deref().or(c.region.as_deref()).map(encoded);
            let path = match c.service.as_str() {
                "sql" | "sql-instances" => Some(format!("sql/instances/{resource}/overview")),
                "buckets" => Some(format!("storage/browser/{resource}")),
                "builds" | "build-details" => region
                    .as_ref()
                    .map(|region| format!("cloud-build/builds;region={region}/{resource}")),
                "instances" | "compute-instances" => c.zone.as_deref().map(|zone| {
                    format!(
                        "compute/instancesDetail/zones/{}/instances/{resource}",
                        encoded(zone)
                    )
                }),
                "run" | "cloud-run" => region
                    .as_ref()
                    .map(|region| format!("run/detail/{region}/{resource}/metrics")),
                "clusters" => location.as_ref().map(|region| {
                    format!("kubernetes/clusters/details/{region}/{resource}/details")
                }),
                "topics" | "pubsub-topics" => Some(format!("cloudpubsub/topic/detail/{resource}")),
                "subscriptions" | "pubsub-subscriptions" => {
                    Some(format!("cloudpubsub/subscription/detail/{resource}"))
                }
                "redis" => region.as_ref().map(|region| {
                    format!("memorystore/redis/locations/{region}/instances/{resource}/details")
                }),
                "pods" => match (&location, &c.cluster, &c.namespace, &c.name) {
                    (Some(region), Some(cluster), Some(namespace), Some(name)) => Some(format!(
                        "kubernetes/pod/{region}/{}/{}/{}/details",
                        encoded(cluster),
                        encoded(namespace),
                        encoded(name)
                    )),
                    _ => None,
                },
                "deployments" | "statefulsets" | "daemonsets" | "jobs" | "cronjobs" => {
                    match (&location, &c.cluster, &c.namespace, &c.name) {
                        (Some(location), Some(cluster), Some(namespace), Some(name)) => {
                            Some(format!(
                                "kubernetes/{}/{}/{}/{}/{}/overview",
                                c.service.trim_end_matches('s'),
                                location,
                                encoded(cluster),
                                encoded(namespace),
                                encoded(name)
                            ))
                        }
                        _ => None,
                    }
                }
                "dns-zones" => Some(format!("net-services/dns/zones/{resource}/details")),
                "secrets" => Some(format!("security/secret-manager/secret/{resource}/details")),
                "alert-policies" => Some(format!("monitoring/alerting/policies/{resource}")),
                "scheduler" => region
                    .as_ref()
                    .map(|region| format!("cloudscheduler/jobs/edit/{region}/{resource}")),
                _ => None,
            };
            match path {
                Some(path) => (
                    "Open in Google Cloud",
                    format!("https://console.cloud.google.com/{path}?project={project}"),
                ),
                None => (
                    "Find in Google Cloud",
                    format!(
                        "https://console.cloud.google.com/search?project={project}&q={}",
                        encoded(&c.native_id)
                    ),
                ),
            }
        }
        Provider::Aws => {
            let region = c.region.as_deref().filter(|r| {
                r.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            });
            let host = region.map_or("https://console.aws.amazon.com".into(), |r| {
                format!("https://{r}.console.aws.amazon.com")
            });
            let id = encoded(&c.native_id);
            let path = match c.service.as_str() {
                "instances" | "ec2" => Some(format!(
                    "ec2/home#InstanceDetails:instanceId={}",
                    encoded(name)
                )),
                "volumes" | "ebs" => {
                    Some(format!("ec2/home#VolumeDetails:volumeId={}", encoded(name)))
                }
                "buckets" | "s3" => Some(format!("s3/buckets/{}", encoded(name))),
                "functions" | "lambda" => Some(format!(
                    "lambda/home#/functions/{}",
                    encoded(c.native_id.rsplit(':').next().unwrap_or(name))
                )),
                "eks" | "clusters" => Some(format!("eks/home#/clusters/{}", encoded(name))),
                "ecs-clusters" => Some(format!("ecs/v2/clusters/{}", encoded(name))),
                "ecr" => Some(format!(
                    "ecr/repositories/private/{}/{}",
                    encoded(&c.scope),
                    encoded(name)
                )),

                "rds" | "rds-instances" | "databases" => Some(format!(
                    "rds/home#database:id={};is-cluster=false",
                    encoded(c.native_id.rsplit(':').next().unwrap_or(name))
                )),
                "sqs" | "sqs-attributes" => Some(format!("sqs/v3/home#/queues/{id}")),
                _ => None,
            };
            match path {
                Some(path) => ("Open in AWS", format!("{host}/{path}")),
                None => ("Open AWS console", format!("{host}/console/home")),
            }
        }
        _ => return vec![],
    };
    vec![Link {
        label: label.into(),
        url: destination,
    }]
}
