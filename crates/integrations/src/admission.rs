//! Remote operations share reloadable scope and process limits, including nested collectors.
use crate::transport::Error;
use monitor_core::{
    budget::{Budget, Permit},
    config::{
        resolve::{Effective, Job},
        settings::Settings,
    },
};
use std::{collections::BTreeMap, future::Future, sync::Arc};
use tokio::sync::RwLock;

pub struct Limits {
    global: Arc<Budget>,
    scopes: RwLock<BTreeMap<String, Arc<Budget>>>,
}
#[derive(Clone)]
pub struct Context {
    global: Arc<Budget>,
    scope: Arc<Budget>,
    settings: Settings,
}
pub struct Reservation {
    _global: Permit,
    _scope: Permit,
}
tokio::task_local! { static CURRENT: Context; }
impl Limits {
    pub fn new(effective: &Effective) -> Self {
        let global = effective
            .jobs
            .first()
            .map_or(1, |job| job.settings.concurrency);
        Self {
            global: Arc::new(Budget::new(global)),
            scopes: RwLock::new(
                effective
                    .jobs
                    .iter()
                    .map(|job| {
                        (
                            job.scope(),
                            Arc::new(Budget::new(job.settings.scope_concurrency)),
                        )
                    })
                    .collect(),
            ),
        }
    }
    /// Old in-flight reservations remain charged when a replacement lowers the limit.
    pub async fn reload(&self, effective: &Effective) {
        self.global.resize(
            effective
                .jobs
                .first()
                .map_or(1, |job| job.settings.concurrency),
        );
        let mut scopes = self.scopes.write().await;
        let mut next = BTreeMap::new();
        for job in &effective.jobs {
            let budget = scopes
                .get(&job.scope())
                .cloned()
                .unwrap_or_else(|| Arc::new(Budget::new(job.settings.scope_concurrency)));
            budget.resize(job.settings.scope_concurrency);
            next.insert(job.scope(), budget);
        }
        *scopes = next;
    }
    pub async fn context(&self, job: &Job) -> Result<Context, Error> {
        let scope = self
            .scopes
            .read()
            .await
            .get(&job.scope())
            .cloned()
            .ok_or(Error::Cancelled)?;
        Ok(Context {
            global: self.global.clone(),
            scope,
            settings: job.settings.clone(),
        })
    }
}
impl Context {
    /// Child futures are polled in this context; collectors do not spawn unbounded tasks.
    pub async fn run<T>(&self, future: impl Future<Output = T>) -> T {
        CURRENT.scope(self.clone(), future).await
    }
}
pub fn settings(fallback: &Settings) -> Settings {
    CURRENT
        .try_with(|context| context.settings.clone())
        .unwrap_or_else(|_| fallback.clone())
}
pub async fn acquire() -> Result<Option<Reservation>, Error> {
    let Ok(context) = CURRENT.try_with(Clone::clone) else {
        return Ok(None);
    };
    // A throttled scope waits without occupying another provider's global capacity.
    let scope = context.scope.acquire().await.map_err(|_| Error::Limit)?;
    let global = context.global.acquire().await.map_err(|_| Error::Limit)?;
    Ok(Some(Reservation {
        _global: global,
        _scope: scope,
    }))
}
/// Conservative fan-out also bounds temporary projected results before they are merged.
pub fn width(settings: &Settings) -> usize {
    settings.scope_concurrency.min(settings.concurrency).max(1)
}
