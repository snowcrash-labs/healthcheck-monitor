//! Native AAD-to-registry token exchange with bounded, transient credentials.
use crate::{auth::Auth, common::Endpoint, registry_tokens::Tokens};
use monitor_core::config::resolve::Job;
use monitor_integrations::{
    projection::text,
    transport::{Error, Http},
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
pub async fn request(
    http: &Http,
    auth: &Auth,
    endpoint: &Endpoint,
    job: &Job,
    cancel: &CancellationToken,
    cache: Option<&Tokens>,
) -> Result<Value, Error> {
    let url = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed)?;
    let host = url.host_str().ok_or(Error::Malformed)?;
    if !host.ends_with(".azurecr.io") || !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Forbidden);
    }
    let scope = if url.path() == "/v2/_catalog" {
        "registry:catalog:*".into()
    } else {
        let repository = url
            .path()
            .strip_prefix("/acr/v1/")
            .and_then(|path| path.strip_suffix("/_manifests"))
            .ok_or(Error::Forbidden)?;
        format!("repository:{repository}:metadata_read,pull")
    };
    let key = format!("azure/{host}/{scope}");
    let cached = match cache {
        Some(cache) => cache.get(&key).await,
        None => None,
    };
    let token = match cached {
        Some(token) => token,
        None => {
            let mut settings = job.settings.clone();
            settings.response_bytes = settings.response_bytes.min(65536);
            let access = auth.registry_bearer().await?;
            let form = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("grant_type", "access_token")
                .append_pair("service", host)
                .append_pair("access_token", &access)
                .finish();
            let request = http
                .client()
                .post(format!("https://{host}/oauth2/exchange"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form)
                .build()
                .map_err(|_| Error::Malformed)?;
            let response = http.json(request, &settings, cancel).await?;
            let refresh = text(&response, &["/refresh_token"]).ok_or(Error::Authentication)?;
            let form = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("grant_type", "refresh_token")
                .append_pair("service", host)
                .append_pair("scope", &scope)
                .append_pair("refresh_token", refresh)
                .finish();
            let request = http
                .client()
                .post(format!("https://{host}/oauth2/token"))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form)
                .build()
                .map_err(|_| Error::Malformed)?;
            let response = http.json(request, &settings, cancel).await?;
            let token = text(&response, &["/access_token"])
                .ok_or(Error::Authentication)?
                .to_owned();
            if let Some(cache) = cache {
                cache.put(key, token.clone(), 300).await;
            }
            token
        }
    };
    let request = http
        .client()
        .get(url)
        .bearer_auth(token)
        .build()
        .map_err(|_| Error::Malformed)?;
    http.json(request, &job.settings, cancel).await
}
