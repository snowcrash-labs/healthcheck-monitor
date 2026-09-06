//! Console links preserve native resource scope and reject malformed destination inputs.
use crate::{console_links::links, log_links};
use monitor_core::{
    diagnostics::ResourceContext,
    model::{Data, LogClass, Provider},
};
fn context(provider: Provider, service: &str, native_id: &str) -> ResourceContext {
    ResourceContext {
        provider,
        scope: "example".into(),
        service: service.into(),
        native_id: native_id.into(),
        region: Some("us-central1".into()),
        zone: Some("us-central1-a".into()),
        cluster: None,
        namespace: None,
        name: None,
        uid: None,
        container: None,
        reason: None,
        exit_code: None,
    }
}
#[test]
fn native_resource_destinations_have_expected_scope_and_encoded_identifiers()
-> Result<(), Box<dyn std::error::Error>> {
    for (c, host, path) in [
        (
            context(
                Provider::Gcp,
                "cloud-run",
                "projects/example/locations/us-central1/services/api",
            ),
            "console.cloud.google.com",
            "/run/detail/us-central1/api/metrics",
        ),
        (
            context(
                Provider::Azure,
                "resource-graph",
                "/subscriptions/example/resourceGroups/rg/providers/Microsoft.Compute/virtualMachines/vm",
            ),
            "portal.azure.com",
            "/",
        ),
        (
            context(Provider::Aws, "instances", "i-012345"),
            "us-central1.console.aws.amazon.com",
            "/ec2/home",
        ),
    ] {
        let rows = links(Some(&c));
        let url = url::Url::parse(&rows.first().ok_or("link")?.url)?;
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some(host));
        assert_eq!(url.path(), path);
    }
    let c = context(Provider::Gcp, "sql", "db?project=evil&token=example");
    let rows = links(Some(&c));
    let url = url::Url::parse(&rows[0].url)?;
    assert_eq!(
        url.query_pairs().collect::<Vec<_>>(),
        vec![("project".into(), "example".into())]
    );
    assert!(links(Some(&context(Provider::Gcp, "sql", "bad\nname"))).is_empty());
    assert!(links(None).is_empty());
    Ok(())
}
#[test]
fn log_queries_keep_source_scope_and_absolute_window_without_payloads()
-> Result<(), Box<dyn std::error::Error>> {
    let mut c = context(Provider::Gcp, "logs", "namespace/pod");
    c.namespace = Some("workers".into());
    c.name = Some("worker-1".into());
    let at = chrono::Utc::now();
    let data = Data::Log {
        signature: LogClass::Panic,
        count: 3,
        first_seen: at,
        last_seen: at,
        sampled: true,
    };
    let rows = log_links::links(Some(&c), &data);
    let url = &rows.first().ok_or("log link")?.url;
    assert!(url.contains("namespace_name"));
    assert!(url.contains("timestamp"));
    assert!(url.ends_with("project=example"));
    Ok(())
}
