//! Independent resource lists share one evidence budget; each continuation remains sequential.
use crate::{
    kubernetes::{KINDS, Kubernetes, required_kind},
    projection::{operation, text},
    transport::Error,
};
use futures::{StreamExt, stream};
use monitor_core::{collection_budget::Limit, config::resolve::Job, model::*};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub trait Source: Send + Sync {
    fn json(
        &self,
        path: &str,
        job: &Job,
        cancel: &CancellationToken,
    ) -> impl std::future::Future<Output = Result<Value, Error>> + Send;
}
impl Source for Kubernetes {
    async fn json(
        &self,
        path: &str,
        job: &Job,
        cancel: &CancellationToken,
    ) -> Result<Value, Error> {
        self.json(path, job, cancel).await
    }
}
struct Collected {
    result: CheckResult,
    budget: Limit,
}
pub async fn collect<S: Source>(source: &S, job: &Job, cancel: &CancellationToken) -> CheckResult {
    let mut result = CheckResult::failure(
        job.target.name.clone(),
        job.check,
        job.revision.clone(),
        Coverage::Missing,
    );
    result.operations.clear();
    let collected = Mutex::new(Collected {
        result,
        budget: Limit::new(&job.settings),
    });
    let mut requests =
        stream::iter((0..KINDS.len()).filter(|index| required_kind(job, KINDS[*index].0)))
            .map(|index| {
                collect_kind(
                    source,
                    job,
                    cancel,
                    KINDS[index].0,
                    KINDS[index].1,
                    &collected,
                )
            })
            .buffer_unordered(crate::admission::width(&job.settings));
    while let Some(op) = requests.next().await {
        let mut guard = collected.lock().await;
        let Collected { result, budget } = &mut *guard;
        budget.operations(&mut result.operations, [op]);
    }
    drop(requests);
    let Collected {
        mut result,
        mut budget,
    } = collected.into_inner();
    budget.finish(&mut result, job.settings.required);
    result.operations.sort_by(|a, b| a.id.cmp(&b.id));
    result
        .observations
        .sort_by(|a, b| a.resource.cmp(&b.resource));
    result.finished_at = chrono::Utc::now();
    result
}
async fn collect_kind<S: Source>(
    source: &S,
    job: &Job,
    cancel: &CancellationToken,
    kind: &str,
    api: &str,
    collected: &Mutex<Collected>,
) -> Operation {
    let mut token = String::new();
    let mut count = 0;
    let mut outcome = Ok(0);
    let mut pages = 0;
    for page in 0..job.settings.max_pages {
        if cancel.is_cancelled() {
            outcome = Err(Error::Cancelled);
            break;
        }
        pages = page + 1;
        let query = {
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            query
                .append_pair("limit", &job.settings.page_size.to_string())
                .append_pair("continue", &token);
            if kind == "events" {
                query.append_pair("fieldSelector", "type=Warning");
            }
            query.finish()
        };
        match source.json(&format!("{api}?{query}"), job, cancel).await {
            Ok(payload) => {
                let Some(items) = payload.get("items").and_then(Value::as_array) else {
                    outcome = Err(Error::Malformed);
                    break;
                };
                {
                    let mut guard = collected.lock().await;
                    let Collected { result, budget } = &mut *guard;
                    for item in items {
                        let before = result.observations.len();
                        if !budget.observations(
                            &mut result.observations,
                            crate::kube_projection::project(job, kind, item),
                        ) {
                            outcome = Err(Error::Limit);
                        }
                        count += result.observations.len() - before;
                        if outcome.is_err() {
                            break;
                        }
                    }
                }
                if outcome.is_err() {
                    break;
                }
                outcome = Ok(count);
                let next = text(&payload, &["/metadata/continue"]).unwrap_or("");
                if next.is_empty() {
                    break;
                }
                if next == token || page + 1 == job.settings.max_pages {
                    outcome = Err(Error::Limit);
                    break;
                }
                token = next.to_string();
            }
            Err(error) => {
                outcome = Err(error);
                break;
            }
        }
    }
    let required =
        !api.contains(".io/") || matches!(outcome, Err(Error::Denied | Error::Authentication));
    operation(
        kind,
        outcome.as_ref().copied(),
        pages,
        required && job.settings.required,
    )
}
