//! Native AWS SDK requests share a bounded reqwest/rustls transport.
use aws_smithy_runtime_api::client::{
    http::{
        HttpConnector, HttpConnectorFuture, SharedHttpClient, SharedHttpConnector, http_client_fn,
    },
    orchestrator::{HttpRequest, HttpResponse},
    result::ConnectorError,
};
use aws_smithy_types::body::SdkBody;
use monitor_core::config::settings::Settings;
use monitor_integrations::transport::{Error, Http, bounded};
#[derive(Clone)]
struct Connector {
    client: reqwest::Client,
    limit: usize,
}
impl std::fmt::Debug for Connector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundedAwsConnector")
    }
}
fn failure() -> ConnectorError {
    ConnectorError::other(Box::new(Error::Unavailable), None)
}
impl HttpConnector for Connector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let connector = self.clone();
        HttpConnectorFuture::new(async move {
            let body = request.body().bytes().ok_or_else(failure)?;
            if body.len() > connector.limit {
                return Err(failure());
            }
            let mut builder = connector
                .client
                .request(
                    reqwest::Method::from_bytes(request.method().as_bytes())
                        .map_err(|_| failure())?,
                    request.uri(),
                )
                .body(body.to_vec());
            for (name, value) in request.headers().iter() {
                builder = builder.header(name, value);
            }
            let response = builder.send().await.map_err(|_| failure())?;
            let mut converted = http::Response::builder().status(response.status());
            for (name, value) in response.headers() {
                converted = converted.header(name, value);
            }
            let bytes = bounded(response, connector.limit)
                .await
                .map_err(|_| failure())?;
            let response = converted
                .body(SdkBody::from(bytes))
                .map_err(|_| failure())?;
            HttpResponse::try_from(response).map_err(|_| failure())
        })
    }
}
pub fn client(http: &Http, settings: &Settings) -> SharedHttpClient {
    let connector = SharedHttpConnector::new(Connector {
        client: http.client().clone(),
        limit: settings.response_bytes,
    });
    http_client_fn(move |_, _| connector.clone())
}
