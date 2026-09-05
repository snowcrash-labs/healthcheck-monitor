//! Allowlisted owner links, scaler metadata, and advertised public endpoints.
use super::projection::{identity, number, observation, text};
use monitor_core::{config::resolve::Job, model::*};
use serde_json::Value;
pub fn project(job: &Job, kind: &str, name: &str, value: &Value, out: &mut Vec<Observation>) {
    if let Some(uid) = text(value, &["/metadata/uid"]) {
        let owner = value
            .pointer("/metadata/ownerReferences")
            .and_then(Value::as_array)
            .and_then(|owners| {
                owners
                    .iter()
                    .find(|o| o.get("controller").and_then(Value::as_bool) == Some(true))
            })
            .and_then(|o| text(o, &["/uid"]));
        out.push(observation(
            job,
            kind,
            &format!("{name}/owner"),
            Data::Owner {
                uid: identity(uid),
                owner_uid: owner.map(identity),
            },
        ));
    }
    if kind == "scaledobjects" {
        let namespace = text(value, &["/metadata/namespace"]).unwrap_or("default");
        let scaler = text(value, &["/metadata/name"]).unwrap_or("");
        let worker = text(value, &["/spec/scaleTargetRef/name"]).unwrap_or("");
        let ready = value
            .pointer("/status/conditions")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.iter().any(|r| {
                    text(r, &["/type"]) == Some("Ready") && text(r, &["/status"]) == Some("True")
                })
            });
        if let Some(metrics) = value
            .pointer("/status/externalMetricNames")
            .and_then(Value::as_array)
        {
            for (index, metric) in metrics
                .iter()
                .filter_map(Value::as_str)
                .take(32)
                .enumerate()
            {
                let trigger = value
                    .pointer("/spec/triggers")
                    .and_then(Value::as_array)
                    .and_then(|a| a.get(index));
                let activation = trigger
                    .and_then(|t| {
                        number(
                            t,
                            &[
                                "/metadata/activationLagCount",
                                "/metadata/activationThreshold",
                                "/metadata/activationQueueLength",
                            ],
                        )
                    })
                    .unwrap_or(0.0);
                out.push(observation(
                    job,
                    kind,
                    &format!("{name}/metric/{index}"),
                    Data::Scaler {
                        namespace: identity(namespace),
                        name: identity(scaler),
                        worker: identity(worker),
                        metric: identity(metric),
                        activation,
                        ready,
                    },
                ));
            }
        }
    }
    if !matches!(kind, "ingresses" | "httproutes" | "mappings") {
        return;
    }
    let mut hosts = Vec::new();
    if let Some(rules) = value.pointer("/spec/rules").and_then(Value::as_array) {
        hosts.extend(rules.iter().filter_map(|r| text(r, &["/host"])));
    }
    if let Some(names) = value.pointer("/spec/hostnames").and_then(Value::as_array) {
        hosts.extend(names.iter().filter_map(Value::as_str));
    }
    if let Some(host) = text(value, &["/spec/hostname"]) {
        hosts.push(host);
    }
    for host in hosts.into_iter().take(64) {
        if host.contains('*')
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
        {
            continue;
        }
        out.push(observation(
            job,
            kind,
            &format!("{name}/endpoint/{host}"),
            Data::AdvertisedEndpoint {
                url: format!("https://{host}/"),
            },
        ));
    }
}
