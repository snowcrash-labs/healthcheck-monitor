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
        HttpConnectorFuture::new(async {
            let status = aws_smithy_runtime_api::http::StatusCode::try_from(400).map_err(|_| {
                aws_smithy_runtime_api::client::result::ConnectorError::other(
                    Box::new(std::io::Error::other("fixture status")),
                    None,
                )
            })?;
            Ok(HttpResponse::new(status, SdkBody::from("{}")))
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
    let _ = crate::aws_metrics::aws(&auth, &job, &tokio_util::sync::CancellationToken::new()).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    if let crate::auth::Auth::Aws(clients) = &auth {
        let _ = clients.sts.get_caller_identity().send().await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(allowed.load(Ordering::SeqCst));
    Ok(())
}
