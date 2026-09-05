//! Operational metadata, metadata-only Key Vault reads, and bounded policy projection.
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::{Error, allowed};
use monitor_providers::{
    common::{Endpoint, Source, collect_from},
    details::followups,
    resource_projection::project,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
fn job(provider: &str) -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse(&format!("version=1\n[[targets]]\nname='test'\nprovider='{provider}'\nscope='subscription'\nregions=['us-east-1']"))?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Inventory).ok_or_else(||"missing job".into())
}
#[test]
fn key_vault_lists_metadata_and_versions_without_reading_values()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("azure")?;
    let parent = Endpoint::get(
        "key-vaults",
        "https://management.azure.com/subscriptions/subscription/vaults",
        "/value",
    );
    let endpoints = followups(
        &job,
        &parent,
        &json!({"name":"vault","id":"/subscriptions/subscription/resourceGroups/rg/providers/Microsoft.KeyVault/vaults/vault","properties":{"vaultUri":"https://vault.vault.azure.net/"}}),
    );
    let secrets = endpoints
        .iter()
        .find(|endpoint| endpoint.id.starts_with("kv-secrets/"))
        .ok_or("no secrets metadata")?;
    assert!(allowed(&reqwest::Client::new().get(&secrets.url).build()?));
    let versions = followups(
        &job,
        secrets,
        &json!({"id":"https://vault.vault.azure.net/secrets/credential"}),
    );
    assert_eq!(versions.len(), 1);
    assert!(versions[0].url.contains("/secrets/credential/versions?"));
    assert!(allowed(
        &reqwest::Client::new().get(&versions[0].url).build()?
    ));
    assert!(!allowed(
        &reqwest::Client::new()
            .get("https://vault.vault.azure.net/secrets/credential/abcd?api-version=2025-07-01")
            .build()?
    ));
    assert!(
        followups(
            &job,
            secrets,
            &json!({"id":"https://other.vault.azure.net/secrets/credential"})
        )
        .is_empty()
    );
    let observations = project(
        &job,
        &versions[0],
        &json!({"id":"https://vault.vault.azure.net/secrets/credential/abcd","attributes":{"enabled":true,"exp":1800000000},"value":"private-secret","tags":{"owner":"private-customer"}}),
    );
    assert!(matches!(
        observations[0].data,
        Data::KeyMetadata {
            enabled: Some(true),
            expires_at: Some(_),
            ..
        }
    ));
    assert!(!serde_json::to_string(&observations)?.contains("private-"));
    Ok(())
}
#[test]
fn metadata_respects_resource_selection_and_keeps_encryption_and_key_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job("aws")?;
    let disk = Endpoint::get("ebs/us-east-1", "https://ec2.us-east-1.amazonaws.com/", "");
    let values = project(
        &job,
        &disk,
        &json!({"volumeId":"vol-123","status":"in-use","encrypted":true}),
    );
    assert!(matches!(
        values[0].data,
        Data::Service {
            state: ServiceState::Ready,
            encrypted: Some(true),
            ..
        }
    ));
    let key = Endpoint::get(
        "kms-detail/us-east-1",
        "https://kms.us-east-1.amazonaws.com/",
        "",
    );
    let value =
        json!({"KeyId":"key-123","KeyState":"PendingDeletion","KeyUsage":"ENCRYPT_DECRYPT"});
    assert!(
        project(&job, &key, &value)
            .iter()
            .any(|observation| matches!(
                observation.data,
                Data::KeyMetadata {
                    enabled: Some(false),
                    ..
                }
            ))
    );
    job.target.resources = vec!["other-resource".into()];
    assert!(project(&job, &key, &value).is_empty());
    Ok(())
}
struct PolicySource;
impl Source for PolicySource {
    async fn request(&self, _: &Endpoint, _: &Job, _: &CancellationToken) -> Result<Value, Error> {
        Ok(
            json!({"items":[{"name":"firewall","direction":"INGRESS","sourceRanges":["0.0.0.0/0"],"allowed":vec![json!({"IPProtocol":"tcp","ports":["443"]});33]}]}),
        )
    }
}
#[tokio::test]
async fn network_metadata_caps_produce_incomplete_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    let result = collect_from(
        &PolicySource,
        &job("gcp")?,
        vec![Endpoint::get(
            "firewalls",
            "https://compute.googleapis.com/firewalls",
            "/items",
        )],
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(result.operations[0].coverage, Coverage::Truncated);
    assert!(result.observations.iter().any(|observation|matches!(&observation.data,Data::NetworkPolicy{allows_public:Some(true),protocols,..} if protocols.len()==32)));
    Ok(())
}
#[test]
fn route53_uses_zone_id_instead_of_dns_name() -> Result<(), Box<dyn std::error::Error>> {
    let mut parent = Endpoint::get(
        "route53/us-east-1",
        "https://route53.amazonaws.com/2013-04-01/hostedzone",
        "",
    );
    parent.aws = Some(("route53".into(), "us-east-1".into(), String::new()));
    let details = followups(
        &job("aws")?,
        &parent,
        &json!({"Id":"/hostedzone/Z123","Name":"example.com."}),
    );
    assert!(
        details
            .iter()
            .any(|endpoint| endpoint.url.contains("/hostedzone/Z123/rrset"))
    );
    Ok(())
}
