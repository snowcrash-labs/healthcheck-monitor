//! Activity reads select metadata, stay within subscription scope and honor configured record caps.
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::common::{Endpoint, Source};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
struct Activity;
impl Source for Activity {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        let url: url::Url = endpoint.url.parse().map_err(|_| Error::Malformed)?;
        let query: std::collections::BTreeMap<_, _> = url.query_pairs().collect();
        assert_eq!(
            query.get("$select").map(|value| value.as_ref()),
            Some("eventDataId,eventTimestamp,resourceId,operationName,status,level")
        );
        assert!(query.get("$filter").is_some_and(
            |value| value.contains("eventTimestamp ge") && value.contains("eventTimestamp le")
        ));
        Ok(
            json!({"value":(0..3).map(|index|json!({"eventDataId":format!("event-{index}"),"eventTimestamp":"2026-09-05T10:00:00Z","resourceId":"/subscriptions/subscription/resourceGroups/group/providers/Microsoft.Compute/virtualMachines/vm","operationName":{"value":"Microsoft.Compute/virtualMachines/write"},"status":{"value":"Failed"},"caller":"private-email","claims":{"token":"private-token"},"properties":{"requestbody":"private-body"}})).collect::<Vec<_>>()}),
        )
    }
}
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='azure'\nprovider='azure'\nscope='subscription'\nregions=['eastus']")?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Inventory).ok_or_else(||"missing job".into())
}
#[tokio::test]
async fn activity_cap_preserves_metadata_and_marks_incomplete_coverage()
-> Result<(), Box<dyn std::error::Error>> {
    let mut job = job()?;
    job.settings.log_entries = 2;
    let result =
        monitor_providers::azure_activity::collect(&Activity, &job, &CancellationToken::new())
            .await;
    assert_eq!(result.observations.len(), 2);
    assert_eq!(result.operations[0].coverage, Coverage::Truncated);
    assert!(matches!(
        result.observations[0].data,
        Data::Activity {
            state: ServiceState::Failed,
            event_at: Some(_),
            ..
        }
    ));
    assert!(!result.observations[0].data.is_health_evidence());
    assert!(!serde_json::to_string(&result)?.contains("private-"));
    Ok(())
}
#[test]
fn compute_network_storage_and_sql_quotas_are_registered() -> Result<(), Box<dyn std::error::Error>>
{
    let job = job()?;
    let endpoints = monitor_providers::azure::endpoints(&job);
    for namespace in [
        "Microsoft.Compute",
        "Microsoft.Network",
        "Microsoft.Storage",
        "Microsoft.Sql",
    ] {
        assert!(
            endpoints
                .iter()
                .any(|endpoint| endpoint.id.starts_with("quotas/")
                    && endpoint.url.contains(namespace))
        );
    }
    let outside = json!({"resourceId":"/subscriptions/other/resourceGroups/group/providers/Microsoft.Compute/virtualMachines/vm","operationName":{"value":"read"}});
    assert!(
        monitor_providers::resource_projection::project(
            &job,
            &Endpoint::get("activity", "https://management.azure.com/", ""),
            &outside
        )
        .is_empty()
    );
    Ok(())
}
