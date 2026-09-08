//! Each discovery root selects its own native credential profile and remains inventory-only.
use crate::{
    auth::Auth,
    router::{Router, Scope, base},
};
use monitor_core::{config::resolve::Job, model::*};
use monitor_integrations::{projection::operation, transport::Error};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
impl Router {
    pub(crate) async fn discover(
        &self,
        job: &Job,
        scope: &Scope,
        cancel: &CancellationToken,
    ) -> CheckResult {
        let config = self.config.read().await.clone();
        let mut result = base(job);
        for root in config
            .discovery
            .iter()
            .filter(|root| root.provider == job.target.provider)
        {
            let credential = root.credential.as_ref().or(job.target.credential.as_ref());
            let mut profile = credential
                .and_then(|name| config.credentials.get(name))
                .cloned();
            if root.provider == Provider::Azure {
                let profile = profile.get_or_insert(monitor_core::config::types::Credential {
                    google_federation: None,
                    provider: Provider::Azure,
                    credential_file: None,
                    profile: None,
                    tenant: None,
                    role_arn: None,
                    expected_identity: None,
                    token_env: None,
                });
                profile.tenant = Some(root.scope.clone());
            }
            let key = format!("{:?}/{}/{credential:?}", root.provider, root.scope);
            let slot = {
                let (_, entry) = self
                    .discovery_auth
                    .entry_async(key)
                    .await
                    .or_put_with(|| Arc::new(Mutex::new(None)));
                entry.get().clone()
            };
            let mut cached = slot.lock().await;
            if cached.is_none() {
                let account = (root.provider == Provider::Aws
                    && root.scope.len() == 12
                    && root.scope.bytes().all(|byte| byte.is_ascii_digit()))
                .then_some(root.scope.as_str());
                let loaded = tokio::select! {_ =cancel.cancelled()=>Err(Error::Cancelled),loaded=tokio::time::timeout(job.settings.operation_timeout.duration(),Auth::new(root.provider,profile.as_ref(),job.target.regions.first().map(String::as_str),account,&scope.http,&job.settings,&self.processes))=>loaded.map_err(|_|Error::Timeout).and_then(|result|result)};
                match loaded {
                    Ok(auth) => *cached = Some(Arc::new(auth)),
                    Err(error) => {
                        result.operations.push(operation(
                            &format!("discovery-auth/{}", root.scope),
                            Err(&error),
                            0,
                            true,
                        ));
                        continue;
                    }
                }
            }
            let Some(auth) = cached.clone() else {
                continue;
            };
            drop(cached);
            let collected = crate::discovery::collect(
                &scope.http,
                &auth,
                job,
                std::slice::from_ref(root),
                cancel,
            )
            .await;
            if collected
                .operations
                .iter()
                .any(|operation| operation.coverage == Coverage::Unauthenticated)
            {
                *slot.lock().await = None;
            }
            result.operations.extend(collected.operations);
            result.observations.extend(collected.observations);
        }
        if result.operations.is_empty() {
            result
                .operations
                .push(operation("discovery-roots", Err(&Error::Missing), 0, true));
        }
        result.finished_at = chrono::Utc::now();
        result
    }
}
