//! Wire-level safety and response limits are independent of provider projections.
use monitor_integrations::transport::{Error, allowed, bounded};
use reqwest::Client;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
#[test]
fn credential_expiry_and_entitlement_failures_are_not_malformed_payloads() {
    use monitor_integrations::transport::response_error;
    assert!(matches!(
        response_error(
            reqwest::StatusCode::BAD_REQUEST,
            br#"{"__type":"ExpiredTokenException","message":"private-token"}"#
        ),
        Error::Authentication
    ));
    assert!(matches!(
        response_error(
            reqwest::StatusCode::BAD_REQUEST,
            br#"{"__type":"SubscriptionRequiredException"}"#
        ),
        Error::Unavailable
    ));
    assert!(matches!(
        response_error(
            reqwest::StatusCode::FORBIDDEN,
            br#"{"error":{"code":"AuthorizationFailed"}}"#
        ),
        Error::Denied
    ));
}
#[test]
fn secret_access_writes_and_message_consumption_are_forbidden()
-> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    for url in [
        "https://secretmanager.googleapis.com/v1/projects/p/secrets/s/versions/1:access",
        "https://storage.googleapis.com/storage/v1/b/b/objects/customer",
        "https://pubsub.googleapis.com/v1/projects/p/subscriptions/s:pull",
        "https://management.azure.com/subscriptions/a/listKeys",
    ] {
        assert!(!allowed(&client.get(url).build()?));
    }
    for operation in [
        "ReceiveMessage",
        "DeleteMessage",
        "GetSecretValue",
        "PutMetricData",
        "CreateQueue",
    ] {
        assert!(!allowed(
            &client
                .post("https://sqs.us-east-1.amazonaws.com")
                .header("x-amz-target", format!("AmazonSQS.{operation}"))
                .body("{}")
                .build()?
        ));
    }
    assert!(!allowed(
        &client
            .delete("https://api.github.com/repos/org/repo")
            .build()?
    ));
    Ok(())
}
#[test]
fn documented_query_posts_are_read_only() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    for url in [
        "https://logging.googleapis.com/v2/entries:list",
        "https://management.azure.com/providers/Microsoft.ResourceGraph/resources",
        "https://compute.googleapis.com/compute/v1/projects/p/global/backendServices/b/getHealth",
    ] {
        assert!(allowed(&client.post(url).body("{}").build()?));
    }
    assert!(allowed(
        &client
            .post("https://sqs.us-east-1.amazonaws.com")
            .header("x-amz-target", "AmazonSQS.GetQueueAttributes")
            .body("{}")
            .build()?
    ));
    Ok(())
}
#[test]
fn unknown_posts_fail_closed() -> Result<(), Box<dyn std::error::Error>> {
    assert!(!allowed(
        &Client::new()
            .post("https://management.azure.com/arbitrary/action")
            .body("{}")
            .build()?
    ));
    Ok(())
}
#[tokio::test]
async fn response_content_length_is_bounded_before_accumulation()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0; 4096];
        let _ = stream.read(&mut request).await?;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n")
            .await?;
        Ok::<_, std::io::Error>(())
    });
    let response = Client::new()
        .get(format!("http://{address}"))
        .send()
        .await?;
    assert!(matches!(bounded(response, 1024).await, Err(Error::Limit)));
    server.await??;
    Ok(())
}
