//! Native registry authentication uses fixed provider endpoints and never enters evidence.
use crate::{auth::Auth, common::Endpoint};
use monitor_core::config::resolve::Job;
use monitor_integrations::{
    projection::text,
    transport::{Error, Http},
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
pub async fn gcp(
    http: &Http,
    auth: &Auth,
    endpoint: &Endpoint,
    job: &Job,
    cancel: &CancellationToken,
    cache: Option<&crate::registry_tokens::Tokens>,
) -> Result<Value, Error> {
    let url = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed)?;
    let host = url.host_str().ok_or(Error::Malformed)?;
    let repository = url
        .path()
        .strip_prefix("/v2/")
        .and_then(|path| {
            path.split_once("/manifests/")
                .map(|(repository, _)| repository)
        })
        .ok_or(Error::Forbidden)?;
    if !host.ends_with("-docker.pkg.dev")
        || !repository.starts_with(&format!("{}/", job.target.scope))
    {
        return Err(Error::Forbidden);
    }
    let key = format!("gcp/{host}/{repository}");
    let cached = match cache {
        Some(cache) => cache.get(&key).await,
        None => None,
    };
    let token = if let Some(token) = cached {
        token
    } else {
        let token_url = format!("https://{host}/v2/token");
        let request = http
            .client()
            .get(token_url)
            .query(&[
                ("service", host),
                ("scope", &format!("repository:{repository}:pull")),
            ])
            .basic_auth("oauth2accesstoken", Some(auth.bearer().await?))
            .build()
            .map_err(|_| Error::Malformed)?;
        let mut settings = job.settings.clone();
        settings.response_bytes = settings.response_bytes.min(65536);
        let token = http.json(request, &settings, cancel).await?;
        let token = text(&token, &["/token", "/access_token"])
            .ok_or(Error::Authentication)?
            .to_owned();
        if let Some(cache) = cache {
            cache.put(key, token.clone(), 300).await;
        }
        token
    };
    let request=http.client().get(url).bearer_auth(token).header("Accept","application/vnd.oci.image.index.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.v2+json").build().map_err(|_|Error::Malformed)?;
    http.json(request, &job.settings, cancel).await
}
