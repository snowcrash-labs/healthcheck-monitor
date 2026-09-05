//! Subprocess stream bounds, missing helpers, and cancellation cleanup.
use crate::{
    process::{Helper, Processes},
    transport::Error,
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
#[tokio::test]
async fn captures_both_streams() -> Result<(), Box<dyn std::error::Error>> {
    let output = Processes::new(1)
        .run(
            Helper::Fixture {
                script: "printf out; printf err >&2",
            },
            1024,
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await?;
    assert_eq!(output.stdout, b"out");
    assert_eq!(output.stderr, b"err");
    Ok(())
}
#[tokio::test]
async fn missing_executable_is_explicit() {
    let result = Processes::new(1)
        .run(
            Helper::Missing,
            1024,
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(result, Err(Error::Unavailable)));
}
#[tokio::test]
async fn output_limit_cancels_the_process_group() {
    let result = Processes::new(1)
        .run(
            Helper::Fixture {
                script: "while :; do printf '0123456789'; done",
            },
            64,
            Duration::from_secs(5),
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(result, Err(Error::Limit)));
}
#[tokio::test]
async fn cancellation_does_not_wait_for_descendants() -> Result<(), Box<dyn std::error::Error>> {
    let stop = CancellationToken::new();
    let child = stop.clone();
    let task = tokio::spawn(async move {
        Processes::new(1)
            .run(
                Helper::Fixture {
                    script: "sleep 60 & wait",
                },
                1024,
                Duration::from_secs(60),
                &child,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    stop.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), task).await??;
    assert!(matches!(result, Err(Error::Cancelled)));
    Ok(())
}
