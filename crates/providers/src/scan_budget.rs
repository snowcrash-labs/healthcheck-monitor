//! Concurrent inventory branches charge one cumulative budget before retaining metadata.
use monitor_core::{
    collection_budget::{Limit, Shared},
    config::resolve::Job,
};
use std::{future::Future, sync::Arc};
tokio::task_local! { static CURRENT: Arc<Shared>; }
pub async fn run<T>(job: &Job, future: impl Future<Output = T>) -> T {
    CURRENT
        .scope(Arc::new(Shared::new(&job.settings)), future)
        .await
}
pub fn limit(job: &Job) -> Limit {
    CURRENT
        .try_with(|shared| Limit::with_shared(&job.settings, shared.clone()))
        .unwrap_or_else(|_| Limit::new(&job.settings))
}
pub fn claim(bytes: usize) -> bool {
    CURRENT
        .try_with(|shared| shared.claim(bytes))
        .unwrap_or(true)
}
