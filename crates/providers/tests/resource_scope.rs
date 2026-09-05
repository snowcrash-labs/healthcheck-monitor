//! Regional selection controls detailed reads and backend identities remain distinct.
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::{
    common::{Endpoint, Source},
    details::followups,
    resource_projection::project,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
fn job(provider: &str, region: &str) -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse(&format!("version=1\n[[targets]]\nname='test'\nprovider='{provider}'\nscope='project'\nregions=['{region}']"))?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Inventory).ok_or_else(||"missing job".into())
}
#[test]
fn out_of_region_resources_are_inventory_only_and_do_not_trigger_deep_reads()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("gcp", "us-central1")?;
    let endpoint = Endpoint::get(
        "cloud-run/global",
        "https://run.googleapis.com/v2/projects/project/locations/-/services",
        "/services",
    );
    let outside = json!({"name":"projects/project/locations/europe-west1/services/api","latestReadyRevision":"r1","terminalCondition":{"state":"CONDITION_FAILED"}});
    let observations = project(&job, &endpoint, &outside);
    assert!(
        matches!(&observations[0].data,Data::Inventory{family,..} if family=="inventory-only-region")
    );
    assert!(!observations[0].data.is_health_evidence());
    assert!(followups(&job, &endpoint, &outside).is_empty());
    let inside = json!({"name":"vm","zone":"https://www.googleapis.com/compute/v1/projects/project/zones/us-central1-a","status":"RUNNING"});
    assert!(
        project(
            &job,
            &Endpoint::get(
                "instances/global",
                "https://compute.googleapis.com/",
                "/items"
            ),
            &inside
        )
        .iter()
        .any(|observation| matches!(
            observation.data,
            Data::Service {
                state: ServiceState::Ready,
                ..
            }
        ))
    );
    Ok(())
}
#[test]
fn azure_display_locations_and_global_resources_remain_in_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("azure", "eastus")?;
    let endpoint = Endpoint::get(
        "vms",
        "https://management.azure.com/subscriptions/project/vms",
        "/value",
    );
    let value = json!({"id":"/subscriptions/project/resourceGroups/group/providers/Microsoft.Compute/virtualMachines/vm","location":"East US","properties":{"provisioningState":"Succeeded"}});
    assert!(
        project(&job, &endpoint, &value)
            .iter()
            .any(|observation| matches!(
                observation.data,
                Data::Service {
                    state: ServiceState::Ready,
                    ..
                }
            ))
    );
    let global = json!({"id":"/subscriptions/project/resourceGroups/group/providers/Microsoft.Network/dnszones/example.com","location":"global"});
    assert!(!project(&job,&endpoint,&global).iter().any(|observation|matches!(&observation.data,Data::Inventory{family,..} if family=="inventory-only-region")));
    Ok(())
}
struct Outside;
impl Source for Outside {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        assert!(!endpoint.url.contains("/timeSeries"));
        let mut value = json!({});
        value[endpoint.items.trim_start_matches('/')] = if endpoint.id.starts_with("cloud-run/") {
            json!([{"name":"projects/project/locations/europe-west1/services/api"}])
        } else {
            json!([])
        };
        Ok(value)
    }
}
#[tokio::test]
async fn outside_inventory_does_not_register_required_regional_metrics()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job("gcp", "us-central1")?;
    job.check = Check::Metrics;
    let result =
        monitor_providers::gcp_metrics::collect(&Outside, &job, &CancellationToken::new()).await;
    assert!(result.complete());
    assert!(
        result
            .observations
            .iter()
            .all(|observation| !observation.data.is_health_evidence())
    );
    Ok(())
}
#[test]
fn load_balancer_targets_have_individual_identity_and_initial_state_is_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let job = job("aws", "us-east-1")?;
    let endpoint = Endpoint::get(
        "target-health/group",
        "https://elasticloadbalancing.us-east-1.amazonaws.com/",
        "",
    );
    let first = project(
        &job,
        &endpoint,
        &json!({"Target":{"Id":"instance","Port":80},"TargetHealth":{"State":"healthy"}}),
    );
    let second = project(
        &job,
        &endpoint,
        &json!({"Target":{"Id":"instance","Port":443},"TargetHealth":{"State":"initial"}}),
    );
    assert_ne!(first[0].resource, second[0].resource);
    assert!(matches!(
        second[0].data,
        Data::Condition { healthy: None, .. }
    ));
    Ok(())
}
