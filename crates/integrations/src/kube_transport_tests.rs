//! Native Kubernetes transport rejects oversized or failed responses before reading their bodies.
use super::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[tokio::test]
async fn failed_and_oversized_responses_do_not_wait_for_unbounded_bodies()
-> Result<(), Box<dyn std::error::Error>> {
    for (status, expected) in [(403, Coverage::Denied), (200, Coverage::Truncated)] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") && request.len() < 4096 {
                request.push(stream.read_u8().await?);
            }
            stream
                .write_all(
                    format!("HTTP/1.1 {status} Fixture\r\nContent-Length: 67108864\r\n\r\n")
                        .as_bytes(),
                )
                .await?;
            stream.flush().await?;
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            Ok::<_, std::io::Error>(())
        });
        let config = kube::Config::new(format!("http://{address}").parse()?);
        let kube = Kubernetes {
            session: Arc::new(tokio::sync::Mutex::new(crate::kube_auth::Session::new(
                config,
                crate::process::Processes::new(1),
            )?)),
        };
        let job = monitor_core::config::types::Config::parse(
            "version=1\n[[targets]]\nname='kube'\nprovider='kubernetes'\nscope='fixture'",
        )?
        .resolve(&Default::default())?
        .jobs
        .into_iter()
        .next()
        .ok_or("job")?;
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            kube.json("/api/v1/pods", &job, &CancellationToken::new()),
        )
        .await?;
        assert_eq!(result.err().map(|error| error.coverage()), Some(expected));
        server.abort();
        assert!(server.await.is_err_and(|error| error.is_cancelled()));
    }
    Ok(())
}
