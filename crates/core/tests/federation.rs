//! Federation cannot fall back to local profiles or exchange assertions for another provider.
use monitor_core::config::types::Credential;
use serde_json::json;
#[test]
fn explicit_cloud_identity_options_are_validated() -> Result<(), Box<dyn std::error::Error>> {
    let aws = json!({"provider":"aws","role_arn":"arn:aws:iam::123456789012:role/healthcheck-monitor","google_federation":{"subject":"111111111111111111111","audience":"https://health.example/aws/123456789012"}});
    let azure = json!({"provider":"azure","tenant":"11111111-1111-1111-1111-111111111111","google_federation":{"subject":"111111111111111111111","audience":"api://AzureADTokenExchange","client_id":"22222222-2222-2222-2222-222222222222"}});
    for value in [&aws, &azure] {
        serde_json::from_value::<Credential>(value.clone())?.validate()?;
    }
    for (mut value, key, replacement) in [
        (aws.clone(), "profile", json!("default")),
        (
            aws.clone(),
            "role_arn",
            json!("arn:aws:iam::123456789012:user/operator"),
        ),
        (aws, "provider", json!("gcp")),
        (azure.clone(), "tenant", json!("../other")),
        (
            azure,
            "google_federation",
            json!({"subject":"111111111111111111111","audience":"https://other.example","client_id":"22222222-2222-2222-2222-222222222222"}),
        ),
    ] {
        value[key] = replacement;
        assert!(
            serde_json::from_value::<Credential>(value)?
                .validate()
                .is_err()
        );
    }
    Ok(())
}
