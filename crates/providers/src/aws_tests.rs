//! Exercise real SDK serialization through a deterministic connector.
use aws_smithy_runtime_api::client::{
    http::{HttpConnector, HttpConnectorFuture, SharedHttpConnector, http_client_fn},
    orchestrator::{HttpRequest, HttpResponse},
};
use aws_smithy_types::body::SdkBody;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
#[derive(Debug, Clone)]
struct Recorder {
    calls: Arc<AtomicUsize>,
    allowed: Arc<AtomicBool>,
}
impl HttpConnector for Recorder {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut check = reqwest::Client::new()
            .request(reqwest::Method::POST, request.uri())
            .body(request.body().bytes().unwrap_or(&[]).to_vec());
        for (name, value) in request.headers().iter() {
            check = check.header(name, value);
        }
        if !check.build().is_ok_and(|request| {
            monitor_integrations::transport::allowed(&request)
                || crate::aws_transport::authentication(&request)
        }) {
            self.allowed.store(false, Ordering::SeqCst);
        }
        let metrics =
            request.uri().contains("/GetMetricData") || request.uri().contains("/ListMetrics");
        HttpConnectorFuture::new(async move {
            let status =
                aws_smithy_runtime_api::http::StatusCode::try_from(if metrics { 200 } else { 400 })
                    .map_err(|_| {
                        aws_smithy_runtime_api::client::result::ConnectorError::other(
                            Box::new(std::io::Error::other("fixture status")),
                            None,
                        )
                    })?;
            let mut response = HttpResponse::new(
                status,
                if metrics {
                    SdkBody::from(vec![0xa0])
                } else {
                    SdkBody::from("{}")
                },
            );
            if metrics {
                response
                    .headers_mut()
                    .insert("content-type", "application/cbor");
                response
                    .headers_mut()
                    .insert("smithy-protocol", "rpc-v2-cbor");
            }
            Ok(response)
        })
    }
}
#[tokio::test]
async fn cloudwatch_batches_use_read_only_sdk_requests() -> Result<(), Box<dyn std::error::Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let allowed = Arc::new(AtomicBool::new(true));
    let connector = SharedHttpConnector::new(Recorder {
        calls: calls.clone(),
        allowed: allowed.clone(),
    });
    let config = aws_config::SdkConfig::builder()
        .behavior_version(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(
            aws_credential_types::provider::SharedCredentialsProvider::new(
                aws_credential_types::Credentials::new(
                    "synthetic-key",
                    "synthetic-secret",
                    None,
                    None,
                    "fixture",
                ),
            ),
        )
        .disable_request_compression(true)
        .retry_config(aws_config::retry::RetryConfig::standard().with_max_attempts(1))
        .http_client(http_client_fn(move |_, _| connector.clone()))
        .build();
    let auth = crate::auth::Auth::Aws(Box::new(crate::aws_clients::AwsClients::new(config)));
    let config = monitor_core::config::types::Config::parse(
        "version=1\n[[targets]]\nname='test'\nprovider='aws'\nscope='123456789012'\nregions=['us-east-1']",
    )?;
    let mut job = config
        .resolve(&Default::default())?
        .jobs
        .into_iter()
        .find(|j| j.check == monitor_core::model::Check::Metrics)
        .ok_or("missing job")?;
    job.target.metrics = (0..501)
        .map(|i| monitor_core::config::types::MetricQuery {
            aggregation: Default::default(),
            name: format!("m{i}"),
            namespace: "AWS/EC2".into(),
            metric: "CPUUtilization".into(),
            resource: format!("instance-{i}"),
            dimensions: Default::default(),
            capacity: Some(100.0),
            warning: None,
            error: None,
        })
        .collect();
    let result =
        crate::aws_metrics::aws(&auth, &job, &tokio_util::sync::CancellationToken::new()).await;
    assert_eq!(result.operations.len(), 501);
    assert!(
        result
            .operations
            .iter()
            .all(|operation| operation.coverage == monitor_core::model::Coverage::Missing)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    if let crate::auth::Auth::Aws(clients) = &auth {
        let _ = clients.sts.get_caller_identity().send().await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    if let crate::auth::Auth::Aws(clients) = &auth {
        let client = clients.signals("us-east-1", &job.settings).await?;
        let _ = client
            .list_service_level_objectives()
            .include_linked_accounts(false)
            .send()
            .await;
        let _ = client
            .batch_get_service_level_objective_budget_report()
            .slo_ids("example")
            .timestamp(aws_smithy_types::DateTime::from_secs(1800000000))
            .send()
            .await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    if let crate::auth::Auth::Aws(clients) = &auth {
        let client = clients.cloudwatch("us-east-1", &job.settings).await?;
        let expected = std::collections::BTreeSet::from(["AWS/SQS".into()]);
        let (queries, operations) =
            crate::aws_metric_discovery::discover(&client, &job, "us-east-1", Some(&expected))
                .await;
        assert!(queries.is_empty());
        assert_eq!(operations.len(), 1);
        assert_eq!(
            operations[0].coverage,
            monitor_core::model::Coverage::Missing
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    assert!(allowed.load(Ordering::SeqCst));
    Ok(())
}
