//! AWS cursor names differ by protocol and must preserve every selected page.
use monitor_core::{
    config::{
        resolve::{Job, Selection},
        types::Config,
    },
    model::*,
};
use monitor_integrations::transport::Error;
use monitor_providers::common::{Endpoint, Source, collect_from};
use serde_json::{Value, json};
use std::{collections::VecDeque, sync::Mutex};
use tokio_util::sync::CancellationToken;
struct Pages {
    responses: Mutex<VecDeque<Value>>,
    requests: Mutex<Vec<Endpoint>>,
}
impl Source for Pages {
    async fn request(
        &self,
        endpoint: &Endpoint,
        _: &Job,
        _: &CancellationToken,
    ) -> Result<Value, Error> {
        self.requests
            .lock()
            .map_err(|_| Error::Unavailable)?
            .push(endpoint.clone());
        self.responses
            .lock()
            .map_err(|_| Error::Unavailable)?
            .pop_front()
            .ok_or(Error::Missing)
    }
}
fn job() -> Result<Job, Box<dyn std::error::Error>> {
    Config::parse("version=1\n[[targets]]\nname='test'\nprovider='aws'\nscope='123456789012'\nregions=['us-east-1']")?.resolve(&Selection::default())?.jobs.into_iter().find(|job|job.check==Check::Inventory).ok_or_else(||"missing job".into())
}
async fn pages(
    service: &str,
    target: &str,
    first: Value,
    path: &str,
) -> Result<(CheckResult, Vec<Endpoint>), Box<dyn std::error::Error>> {
    let mut first = first;
    first["items"] = json!([{"name":"first","state":"RUNNING"}]);
    let source = Pages {
        responses: Mutex::new(VecDeque::from([
            first,
            json!({"items":[{"name":"second","state":"RUNNING"}]}),
        ])),
        requests: Mutex::new(vec![]),
    };
    let mut endpoint = Endpoint::get(
        "fixture",
        format!("https://{service}.amazonaws.com{path}"),
        "/items",
    );
    endpoint.aws = Some((service.into(), "us-east-1".into(), target.into()));
    if !target.is_empty() {
        endpoint.body = Some(json!({}));
    }
    let result = collect_from(&source, &job()?, vec![endpoint], &CancellationToken::new()).await;
    let requests = source
        .requests
        .into_inner()
        .map_err(|_| "poisoned requests")?;
    Ok((result, requests))
}
#[tokio::test]
async fn query_and_json_cursors_use_their_documented_request_fields()
-> Result<(), Box<dyn std::error::Error>> {
    for (service, target, payload, field) in [
        (
            "autoscaling",
            "query:DescribeAutoScalingGroups",
            json!({"DescribeAutoScalingGroupsResult":{"NextToken":"opaque"}}),
            "NextToken",
        ),
        (
            "ec2",
            "query:DescribeInstances",
            json!({"nextToken":"opaque"}),
            "NextToken",
        ),
        (
            "rds",
            "query:DescribeDBInstances",
            json!({"DescribeDBInstancesResult":{"Marker":"opaque"}}),
            "Marker",
        ),
        (
            "elasticloadbalancing",
            "query:DescribeLoadBalancers",
            json!({"DescribeLoadBalancersResult":{"NextMarker":"opaque"}}),
            "Marker",
        ),
        (
            "dynamodb",
            "DynamoDB_20120810.ListTables",
            json!({"LastEvaluatedTableName":"opaque"}),
            "ExclusiveStartTableName",
        ),
        (
            "kms",
            "TrentService.ListKeys",
            json!({"NextMarker":"opaque","Truncated":true}),
            "Marker",
        ),
        (
            "sns",
            "query:ListTopics",
            json!({"ListTopicsResult":{"NextToken":"opaque"}}),
            "NextToken",
        ),
    ] {
        let (result, requests) = pages(service, target, payload, "/").await?;
        assert!(result.complete(), "{service}");
        assert_eq!(result.observations.len(), 2);
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].body.as_ref().and_then(|body| body.get(field)),
            Some(&json!("opaque")),
            "{service}"
        );
    }
    Ok(())
}
#[tokio::test]
async fn rest_query_names_and_route53_composite_cursor_are_preserved()
-> Result<(), Box<dyn std::error::Error>> {
    for (service, payload, field) in [
        ("eks", json!({"nextToken":"opaque"}), "nextToken"),
        ("backup", json!({"NextToken":"opaque"}), "nextToken"),
        (
            "route53",
            json!({"NextMarker":"opaque","IsTruncated":true}),
            "marker",
        ),
        ("lambda", json!({"NextMarker":"opaque"}), "Marker"),
    ] {
        let (result, requests) = pages(service, "", payload, "/list").await?;
        assert!(result.complete());
        assert!(requests[1].url.contains(&format!("{field}=opaque")));
    }
    let(result,requests)=pages("route53","",json!({"IsTruncated":true,"NextRecordName":"api.example.com.","NextRecordType":"A","NextRecordIdentifier":"west"}),"/2013-04-01/hostedzone/Z123/rrset").await?;
    assert!(result.complete());
    let query: url::Url = requests[1].url.parse()?;
    let pairs: std::collections::BTreeMap<_, _> = query.query_pairs().collect();
    assert_eq!(
        pairs.get("name").map(|value| value.as_ref()),
        Some("api.example.com.")
    );
    assert_eq!(pairs.get("type").map(|value| value.as_ref()), Some("A"));
    assert_eq!(
        pairs.get("identifier").map(|value| value.as_ref()),
        Some("west")
    );
    Ok(())
}
#[tokio::test]
async fn truncated_without_cursor_and_malformed_cursor_are_incomplete()
-> Result<(), Box<dyn std::error::Error>> {
    for payload in [json!({"IsTruncated":true}), json!({"nextToken":123})] {
        let (result, requests) = pages("eks", "", payload, "/clusters").await?;
        assert_eq!(requests.len(), 1);
        assert_eq!(result.operations[0].coverage, Coverage::Malformed);
        assert_eq!(result.observations.len(), 1);
    }
    Ok(())
}
