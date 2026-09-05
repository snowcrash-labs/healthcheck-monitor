//! Provider contracts use deterministic transports without cloud credentials.
use monitor_core::{
    config::{resolve::Selection, types::Config},
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::common::{Endpoint, Source, collect_from};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use tokio_util::sync::CancellationToken;
struct Fake {
    responses: Mutex<VecDeque<Result<Value, Error>>>,
}
impl Source for Fake {
    async fn request(
        &self,
        _: &Endpoint,
        _: &monitor_core::config::resolve::Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        self.responses
            .lock()
            .map_err(|_| Error::Unavailable)?
            .pop_front()
            .ok_or(Error::Missing)?
    }
}
fn job() -> Result<monitor_core::config::resolve::Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='dev'\nprovider='gcp'\nscope='test-project'\nregions=['us-central1']")?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Inventory).ok_or_else(||"missing job".into())
}
fn endpoint() -> Endpoint {
    Endpoint::get(
        "example",
        "https://example.googleapis.com/v1/resources",
        "/items",
    )
}
#[tokio::test]
async fn paginated_inventory_keeps_every_available_page() -> Result<(), Box<dyn std::error::Error>>
{
    let source = Fake {
        responses: Mutex::new(VecDeque::from([
            Ok(json!({"items":[{"name":"a","state":"RUNNING"}],"nextPageToken":"two"})),
            Ok(json!({"items":[{"name":"b","state":"RUNNING"}]})),
        ])),
    };
    let result = collect_from(
        &source,
        &job()?,
        vec![endpoint()],
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(result.observations.len(), 2);
    assert!(result.complete());
    assert_eq!(result.operations[0].pages, 2);
    Ok(())
}
#[tokio::test]
async fn pagination_loops_fail_closed_with_partial_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Fake {
        responses: Mutex::new(VecDeque::from([
            Ok(json!({"items":[{"name":"a"}],"nextPageToken":"same"})),
            Ok(json!({"items":[{"name":"b"}],"nextPageToken":"same"})),
        ])),
    };
    let result = collect_from(
        &source,
        &job()?,
        vec![endpoint()],
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(result.observations.len(), 2);
    assert_eq!(result.operations[0].coverage, Coverage::Truncated);
    Ok(())
}
#[tokio::test]
async fn independent_apis_survive_permission_expiry_throttle_and_malformed_responses()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Fake {
        responses: Mutex::new(VecDeque::from([
            Err(Error::Denied),
            Err(Error::Authentication),
            Err(Error::Throttled),
            Err(Error::Malformed),
            Err(Error::Unavailable),
            Ok(json!({"items":[{"name":"available","state":"RUNNING"}]})),
        ])),
    };
    let endpoints = (0..6)
        .map(|i| {
            Endpoint::get(
                format!("api-{i}"),
                format!("https://example.googleapis.com/{i}"),
                "/items",
            )
        })
        .collect();
    let result = collect_from(&source, &job()?, endpoints, &CancellationToken::new()).await;
    assert_eq!(result.operations.len(), 6);
    assert_eq!(result.observations.len(), 1);
    assert!(!result.complete());
    Ok(())
}
#[tokio::test]
async fn malformed_list_is_not_an_empty_healthy_inventory() -> Result<(), Box<dyn std::error::Error>>
{
    let source = Fake {
        responses: Mutex::new(VecDeque::from([Ok(json!({"items":"broken"}))])),
    };
    let result = collect_from(
        &source,
        &job()?,
        vec![endpoint()],
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(result.operations[0].coverage, Coverage::Malformed);
    Ok(())
}
#[tokio::test]
async fn pagination_cannot_redirect_bearer_credentials_to_another_host()
-> Result<(), Box<dyn std::error::Error>> {
    let source = Fake {
        responses: Mutex::new(VecDeque::from([Ok(
            json!({"items":[{"name":"a"}],"nextLink":"https://attacker.invalid/steal"}),
        )])),
    };
    let result = collect_from(
        &source,
        &job()?,
        vec![endpoint()],
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(result.operations[0].coverage, Coverage::Denied);
    Ok(())
}
#[test]
fn regional_and_global_gcp_build_locations_are_included() -> Result<(), Box<dyn std::error::Error>>
{
    let endpoints = monitor_providers::gcp::endpoints(&job()?);
    assert!(
        endpoints
            .iter()
            .any(|e| e.url.contains("/locations/global/builds"))
    );
    assert!(
        endpoints
            .iter()
            .any(|e| e.url.contains("/locations/us-central1/builds"))
    );
    Ok(())
}
#[test]
fn provider_objects_cannot_leak_unallowlisted_payloads() -> Result<(), Box<dyn std::error::Error>> {
    let value = json!({"name":"instance","state":"RUNNING","password":"private-password","env":[{"value":"customer-data"}],"connectionString":"private-connection"});
    let observations =
        monitor_providers::resource_projection::project(&job()?, &endpoint(), &value);
    let encoded = serde_json::to_string(&observations)?;
    for secret in ["private-password", "customer-data", "private-connection"] {
        assert!(!encoded.contains(secret));
    }
    Ok(())
}
#[test]
fn active_service_events_are_outages_not_healthy_active_resources()
-> Result<(), Box<dyn std::error::Error>> {
    let event = Endpoint::get(
        "provider-health",
        "https://servicehealth.googleapis.com/v1/events",
        "/events",
    );
    let observations = monitor_providers::resource_projection::project(
        &job()?,
        &event,
        &json!({"name":"incident","state":"ACTIVE"}),
    );
    assert!(observations.iter().any(|o| matches!(
        o.data,
        Data::Condition {
            healthy: Some(false),
            ..
        }
    )));
    Ok(())
}
