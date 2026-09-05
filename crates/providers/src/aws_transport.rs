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
    http: Http,
    metadata: reqwest::Client,
    settings: Settings,
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
            let settings = monitor_integrations::admission::settings(&connector.settings);
            let body = request.body().bytes().ok_or_else(failure)?;
            if body.len() > settings.response_bytes {
                return Err(ConnectorError::other(Box::new(Error::Limit), None));
            }
            let metadata = metadata_uri(request.uri(), request.method());
            let client = if metadata {
                connector.metadata.clone()
            } else {
                connector
                    .http
                    .client_for(&settings)
                    .map_err(|error| ConnectorError::other(Box::new(error), None))?
            };
            let mut builder = client
                .request(
                    reqwest::Method::from_bytes(request.method().as_bytes())
                        .map_err(|_| failure())?,
                    request.uri(),
                )
                .body(body.to_vec())
                .timeout(settings.attempt_timeout.duration());
            for (name, value) in request.headers().iter() {
                builder = builder.header(name, value);
            }
            let request = builder.build().map_err(|_| failure())?;
            if !metadata
                && !monitor_integrations::transport::allowed(&request)
                && !authentication(&request)
            {
                return Err(ConnectorError::other(Box::new(Error::Forbidden), None));
            }
            let _permit = monitor_integrations::admission::acquire()
                .await
                .map_err(|error| ConnectorError::other(Box::new(error), None))?;
            let response = client.execute(request).await.map_err(|error| {
                ConnectorError::other(
                    Box::new(if error.is_timeout() {
                        Error::Timeout
                    } else {
                        Error::Unavailable
                    }),
                    None,
                )
            })?;
            let mut converted = http::Response::builder().status(response.status());
            for (name, value) in response.headers() {
                converted = converted.header(name, value);
            }
            let bytes = bounded(response, settings.response_bytes)
                .await
                .map_err(|error| ConnectorError::other(Box::new(error), None))?;
            let response = converted
                .body(SdkBody::from(bytes))
                .map_err(|_| failure())?;
            HttpResponse::try_from(response).map_err(|_| failure())
        })
    }
}
pub(crate) fn authentication(request: &reqwest::Request) -> bool {
    let host = request.url().host_str().unwrap_or("");
    if request.url().scheme() != "https" || !host.ends_with(".amazonaws.com") {
        return false;
    }
    if host.starts_with("oidc.") && request.url().path() == "/token" {
        return true;
    }
    if !(host == "sts.amazonaws.com" || host.starts_with("sts.")) {
        return false;
    }
    if request
        .url()
        .path()
        .strip_prefix("/service/AWSSecurityTokenServiceV20110615/operation/")
        .is_some_and(|operation| {
            matches!(
                operation,
                "GetCallerIdentity" | "AssumeRole" | "AssumeRoleWithWebIdentity"
            )
        })
    {
        return true;
    }
    let action = request
        .body()
        .and_then(|body| body.as_bytes())
        .and_then(|body| {
            url::form_urlencoded::parse(body)
                .find(|(key, _)| key == "Action")
                .map(|(_, value)| value.into_owned())
        });
    action.is_some_and(|action| {
        matches!(
            action.as_str(),
            "GetCallerIdentity" | "AssumeRole" | "AssumeRoleWithWebIdentity"
        )
    })
}
pub fn client(http: &Http, settings: &Settings) -> Result<SharedHttpClient, Error> {
    let metadata = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(settings.connect_timeout.duration())
        .timeout(settings.attempt_timeout.duration())
        .pool_max_idle_per_host(2)
        .build()
        .map_err(|_| Error::Unavailable)?;
    let connector = SharedHttpConnector::new(Connector {
        http: http.clone(),
        metadata,
        settings: settings.clone(),
    });
    Ok(http_client_fn(move |_, _| connector.clone()))
}
fn metadata_uri(uri: &str, method: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    if url.scheme() != "http" {
        return false;
    }
    match url.host_str() {
        Some("169.254.169.254") => {
            method == "PUT" && url.path() == "/latest/api/token"
                || method == "GET"
                    && (url.path().starts_with("/latest/meta-data/iam/")
                        || url.path() == "/latest/dynamic/instance-identity/document")
        }
        Some(
            "169.254.170.2" | "169.254.170.23" | "[fd00:ec2::23]" | "127.0.0.1" | "[::1]"
            | "localhost",
        ) => method == "GET" && url.path().contains("credentials"),
        _ => false,
    }
}
