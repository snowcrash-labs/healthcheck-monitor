//! Native identity projection retains location without unrestricted provider payloads.
use monitor_core::{config::types::Config, model::Check};
use monitor_providers::{common::Endpoint, resource_projection};
use serde_json::json;

#[test]
fn cloud_context_preserves_native_ids_global_locations_and_excludes_sensitive_fields()
-> Result<(), Box<dyn std::error::Error>> {
    for (provider, scope, family, value, identity, region) in [
        (
            "gcp",
            "example",
            "builds/global",
            json!({"id":"build-1","name":"projects/example/locations/global/builds/build-1","status":"FAILURE","createTime":"2026-09-05T00:00:00Z","source":{"repoSource":{"commitSha":"abc1234"}},"secretValue":"FORBIDDEN_SECRET","substitutions":{"PRIVATE":"FORBIDDEN_ENV"}}),
            "projects/example/locations/global/builds/build-1",
            "global",
        ),
        (
            "azure",
            "11111111-1111-1111-1111-111111111111",
            "virtual-machines",
            json!({"id":"/subscriptions/11111111-1111-1111-1111-111111111111/resourceGroups/rg/providers/Microsoft.Compute/virtualMachines/vm","name":"vm","location":"eastus","properties":{"provisioningState":"Failed","secrets":"FORBIDDEN_SECRET","environment":"FORBIDDEN_ENV"}}),
            "/subscriptions/11111111-1111-1111-1111-111111111111/resourceGroups/rg/providers/Microsoft.Compute/virtualMachines/vm",
            "eastus",
        ),
        (
            "aws",
            "123456789012",
            "rds",
            json!({"DBInstanceIdentifier":"database","DBInstanceArn":"arn:aws:rds:us-west-2:123456789012:db:database","DBInstanceStatus":"failed","MasterUserPassword":"FORBIDDEN_SECRET","Environment":"FORBIDDEN_ENV"}),
            "arn:aws:rds:us-west-2:123456789012:db:database",
            "us-west-2",
        ),
    ] {
        let effective = Config::parse(&format!("version=1\n[[targets]]\nname='fixture'\nprovider='{provider}'\nscope='{scope}'\nregions=['us-central1']"))?.resolve(&Default::default())?;
        let job = effective
            .jobs
            .iter()
            .find(|job| job.check == Check::Inventory)
            .ok_or("inventory")?;
        let endpoint = Endpoint::get(
            family,
            "https://cloudbuild.googleapis.com/v1/projects/example/locations/global/builds",
            "/items",
        );
        let observations = resource_projection::project(job, &endpoint, &value);
        assert!(!observations.is_empty());
        let context = observations
            .first()
            .and_then(|observation| observation.context.as_ref())
            .ok_or("context")?;
        assert_eq!(context.native_id, identity);
        assert_eq!(context.region.as_deref(), Some(region));
        let evidence = serde_json::to_string(&observations)?;
        assert!(!evidence.contains("FORBIDDEN_SECRET"));
        assert!(!evidence.contains("FORBIDDEN_ENV"));
    }
    Ok(())
}
