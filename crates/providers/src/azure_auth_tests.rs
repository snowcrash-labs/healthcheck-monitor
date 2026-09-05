//! Native Azure CLI credentials reuse bounded audience-specific tokens and cancellable execution.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Debug)]
struct Recorder(Arc<AtomicUsize>);
impl azure_identity::Executor for Recorder {
    fn run<'s, 'p, 'a, 'v, 'f>(
        &'s self,
        program: &'p OsStr,
        args: &'a [&'v OsStr],
    ) -> Pin<Box<dyn Future<Output = io::Result<std::process::Output>> + Send + 'f>>
    where
        's: 'f,
        'p: 'f,
        'a: 'f,
        'v: 'f,
        Self: 'f,
    {
        Box::pin(async move {
            assert_eq!(program, OsStr::new("/bin/sh"));
            assert!(args.get(1).is_some_and(|value| {
                value
                    .to_string_lossy()
                    .contains("az account get-access-token")
            }));
            self.0.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok(std::process::Output{status:std::process::ExitStatus::from_raw(0),stdout:serde_json::json!({"accessToken":"synthetic-token","tokenType":"Bearer","expires_on":chrono::Utc::now().timestamp()+3600}).to_string().into_bytes(),stderr:vec![]})
        })
    }
}
#[tokio::test]
async fn concurrent_calls_share_one_cli_request_per_audience_and_refresh_expiry()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let native =
        azure_identity::AzureCliCredential::new(Some(azure_identity::AzureCliCredentialOptions {
            executor: Some(Arc::new(Recorder(calls.clone()))),
            ..Default::default()
        }))?;
    let credentials = Credential::new(native);
    for token in
        futures::future::join_all((0..8).map(|_| credentials.bearer(Audience::Management))).await
    {
        assert_eq!(token?, "synthetic-token");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        credentials.bearer(Audience::Registry).await?,
        "synthetic-token"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    if let Some(token) = credentials.cache[0].lock().await.as_mut() {
        token.expires_on = azure_core::time::OffsetDateTime::now_utc();
    }
    let _ = credentials.bearer(Audience::Management).await?;
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    Ok(())
}
#[tokio::test]
async fn executor_caps_streams_and_cancellation_releases_the_shared_budget()
-> Result<(), Box<dyn std::error::Error>> {
    use azure_identity::Executor as _;
    let processes = Processes::new(1);
    let executor = Executor {
        processes: processes.clone(),
        timeout: Duration::from_secs(2),
        limit: 64,
    };
    let args = [
        OsStr::new("-c"),
        OsStr::new("while :; do printf '0123456789'; done"),
    ];
    assert!(executor.run(OsStr::new("/bin/sh"), &args).await.is_err());
    assert_eq!(processes.budget().available_permits(), 1);
    let args = [OsStr::new("-c"), OsStr::new("sleep 60 & wait")];
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            executor.run(OsStr::new("/bin/sh"), &args)
        )
        .await
        .is_err()
    );
    assert_eq!(processes.budget().available_permits(), 1);
    Ok(())
}
