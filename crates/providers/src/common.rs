//! Per-API aggregation preserves unrelated successes when a scan fails.
use crate::auth::Auth;
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{
    projection::{operation, text},
    transport::{Error, Http},
};
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Endpoint {
    pub id: String,
    pub url: String,
    pub items: String,
    pub body: Option<Value>,
    pub aws: Option<(String, String, String)>,
}
impl Endpoint {
    pub fn get(id: impl Into<String>, url: impl Into<String>, items: &str) -> Self {
        Self {
            id: id.into(),
            url: url.into(),
            items: items.into(),
            body: None,
            aws: None,
        }
    }
}
pub async fn request(
    http: &Http,
    auth: &Auth,
    endpoint: &Endpoint,
    job: &Job,
    cancel: &CancellationToken,
) -> Result<Value, Error> {
    let query = endpoint
        .aws
        .as_ref()
        .is_some_and(|(_, _, target)| target.starts_with("query:"));
    let mut builder = if query {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        if let Some(fields) = endpoint.body.as_ref().and_then(Value::as_object) {
            for (name, value) in fields {
                if let Some(value) = value.as_str() {
                    form.append_pair(name, value);
                }
            }
        }
        http.client()
            .post(&endpoint.url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form.finish())
    } else if let Some(body) = &endpoint.body {
        http.client().post(&endpoint.url).json(body)
    } else {
        http.client().get(&endpoint.url)
    };
    if let Some((_, _, target)) = &endpoint.aws {
        if !query && !target.is_empty() {
            builder = builder
                .header("x-amz-target", target)
                .header("content-type", "application/x-amz-json-1.1");
        }
    } else {
        builder = builder.bearer_auth(auth.bearer().await?);
    }
    let mut request = builder.build().map_err(|_| Error::Malformed)?;
    if let Some((service, region, _)) = &endpoint.aws {
        auth.sign(&mut request, service, region).await?;
    }
    http.json(request, &job.settings, cancel).await
}
pub async fn collect(
    http: &Http,
    auth: &Auth,
    job: &Job,
    endpoints: Vec<Endpoint>,
    cancel: &CancellationToken,
) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let mut pending: VecDeque<_> = endpoints.into();
    let mut visited = BTreeSet::new();
    while let Some(endpoint) = pending.pop_front() {
        let request_key = format!("{}:{:?}", endpoint.url, endpoint.body);
        if !visited.insert(request_key) {
            continue;
        }
        if visited.len() > job.settings.max_assets {
            result
                .operations
                .push(operation("discovery-limit", Err(&Error::Limit), 0, true));
            break;
        }
        if cancel.is_cancelled() {
            result.operations.push(operation(
                &endpoint.id,
                Err(&Error::Cancelled),
                0,
                job.settings.required,
            ));
            continue;
        }
        let mut endpoint = endpoint;
        let mut outcome = Ok(0usize);
        let mut pages = 0;
        let mut previous_token = String::new();
        for page in 0..job.settings.max_pages {
            pages = page + 1;
            match request(http, auth, &endpoint, job, cancel).await {
                Ok(payload) => {
                    let rows = crate::resource_projection::rows(&payload, &endpoint.items);
                    if !rows.is_empty() {
                        for row in &rows {
                            if result.observations.len() >= job.settings.max_assets {
                                outcome = Err(Error::Limit);
                                break;
                            }
                            let projected =
                                crate::resource_projection::project(job, &endpoint, row);
                            let available = job
                                .settings
                                .max_assets
                                .saturating_sub(result.observations.len());
                            if projected.len() > available {
                                outcome = Err(Error::Limit);
                            }
                            result
                                .observations
                                .extend(projected.into_iter().take(available));
                            for detail in crate::details::followups(job, &endpoint, row) {
                                if pending.len() >= job.settings.ready_queue {
                                    outcome = Err(Error::Limit);
                                    break;
                                }
                                pending.push_back(detail);
                            }
                        }
                        outcome = outcome.map(|n| n + rows.len());
                    } else if endpoint.items.is_empty() {
                        result
                            .observations
                            .extend(crate::resource_projection::project(
                                job, &endpoint, &payload,
                            ));
                        outcome = Ok(1);
                    } else if payload.as_object().is_some_and(|m| m.is_empty())
                        || payload.pointer(&endpoint.items).is_some_and(|v| {
                            v.as_array().is_some_and(|a| a.is_empty()) || v.as_str() == Some("")
                        })
                    {
                        outcome = Ok(0);
                    } else {
                        outcome = Err(Error::Malformed);
                    }
                    if outcome.is_err() {
                        break;
                    }
                    let token = text(
                        &payload,
                        &[
                            "/nextPageToken",
                            "/nextToken",
                            "/NextToken",
                            "/nextLink",
                            "/NextMarker",
                        ],
                    )
                    .unwrap_or("");
                    if token.is_empty() {
                        break;
                    }
                    if token == previous_token || page + 1 == job.settings.max_pages {
                        outcome = Err(Error::Limit);
                        break;
                    }
                    previous_token = token.into();
                    if token.starts_with("https://") {
                        let next = url::Url::parse(token).map_err(|_| Error::Malformed);
                        let old = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed);
                        match (next, old) {
                            (Ok(next), Ok(old))
                                if next.origin() == old.origin()
                                    && next.path().starts_with(&format!(
                                        "/subscriptions/{}/",
                                        job.target.scope
                                    )) =>
                            {
                                endpoint.url = next.into()
                            }
                            _ => {
                                outcome = Err(Error::Forbidden);
                                break;
                            }
                        }
                    } else if let Some(body) = &mut endpoint.body {
                        let field = if endpoint.aws.is_some() {
                            if payload.get("NextToken").is_some() {
                                "NextToken"
                            } else {
                                "nextToken"
                            }
                        } else {
                            "pageToken"
                        };
                        body[field] = Value::String(token.into());
                    } else {
                        let Ok(mut url) = url::Url::parse(&endpoint.url) else {
                            outcome = Err(Error::Malformed);
                            break;
                        };
                        let field = if job.target.provider == Provider::Gcp {
                            "pageToken"
                        } else {
                            "NextToken"
                        };
                        let pairs: Vec<_> = url
                            .query_pairs()
                            .filter(|(k, _)| k != field)
                            .map(|(k, v)| (k.into_owned(), v.into_owned()))
                            .collect();
                        url.query_pairs_mut()
                            .clear()
                            .extend_pairs(pairs)
                            .append_pair(field, token);
                        endpoint.url = url.into();
                    }
                }
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        result.operations.push(operation(
            &endpoint.id,
            outcome.as_ref().copied(),
            pages,
            job.settings.required,
        ));
    }
    if result.operations.is_empty() {
        result.operations.push(operation(
            "not-configured",
            Err(&Error::Unavailable),
            0,
            job.settings.required,
        ));
    }
    result.finished_at = chrono::Utc::now();
    result
}
