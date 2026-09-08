//! Azure Identity exchanges a Google assertion for each of the monitor's fixed token audiences.
use crate::google_assertion::Assertion;
use azure_core::{credentials::TokenCredential, http::ClientMethodOptions};
use monitor_core::config::{settings::Settings, types::Credential};
use monitor_integrations::transport::{Error, Http};
use std::{future::Future, pin::Pin, sync::Arc};

impl azure_identity::ClientAssertion for Assertion {
    // The SDK trait uses an erased future at its public dynamic-dispatch boundary.
    fn secret<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _: Option<ClientMethodOptions<'life1>>,
    ) -> Pin<Box<dyn Future<Output = azure_core::Result<String>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            self.token().await.map_err(|_| {
                azure_core::Error::new(
                    azure_core::error::ErrorKind::Credential,
                    "Google assertion unavailable",
                )
            })
        })
    }
}
pub(crate) fn credential(
    profile: &Credential,
    http: &Http,
    settings: &Settings,
) -> Result<Arc<dyn TokenCredential>, Error> {
    profile.validate().map_err(|_| Error::Forbidden)?;
    let identity = profile
        .google_federation
        .clone()
        .ok_or(Error::Authentication)?;
    let tenant = profile.tenant.clone().ok_or(Error::Authentication)?;
    let client = identity.client_id.clone().ok_or(Error::Authentication)?;
    let assertion = Assertion::new(identity, settings.attempt_timeout.duration(), false)?;
    azure_identity::ClientAssertionCredential::new(
        tenant,
        client,
        assertion,
        Some(azure_identity::ClientAssertionCredentialOptions {
            client_options: azure_core::http::ClientOptions {
                retry: azure_core::http::RetryOptions::exponential(
                    azure_core::http::ExponentialRetryOptions {
                        max_retries: settings.attempts.saturating_sub(1) as u32,
                        max_total_elapsed: settings
                            .operation_timeout
                            .duration()
                            .try_into()
                            .map_err(|_| Error::Forbidden)?,
                        ..Default::default()
                    },
                ),
                transport: Some(azure_core::http::Transport::new(Arc::new(
                    crate::azure_token_transport::Transport::new(http.client().clone()),
                ))),
                ..Default::default()
            },
        }),
    )
    .map(|credential| credential as Arc<dyn TokenCredential>)
    .map_err(|_| Error::Authentication)
}
