//! Reject credential options that a provider cannot honor.
use super::types::Config;
use crate::{error::Error, model::Provider};
pub fn validate(config: &Config) -> Result<(), Error> {
    for (name, credential) in &config.credentials {
        if !super::validate::identifier(name)
            || credential
                .profile
                .as_ref()
                .is_some_and(|profile| profile.len() > 512 || profile.chars().any(char::is_control))
            || credential
                .expected_identity
                .as_ref()
                .is_some_and(|identity| {
                    identity.len() > 1024 || identity.chars().any(char::is_control)
                })
        {
            return Err(Error::Config("invalid credential profile metadata".into()));
        }
        if credential.tenant.is_some() && credential.provider != Provider::Azure
            || credential.role_arn.is_some() && credential.provider != Provider::Aws
            || credential.credential_file.is_some()
                && !matches!(
                    credential.provider,
                    Provider::Gcp | Provider::Nats | Provider::Github
                )
            || credential.token_env.is_some()
                && !matches!(credential.provider, Provider::Github | Provider::Nats)
        {
            return Err(Error::Config(
                "credential option is unsupported by its provider".into(),
            ));
        }
        if credential.profile.is_some()
            && !matches!(credential.provider, Provider::Aws | Provider::Azure)
        {
            return Err(Error::Config(
                "use credential_file for named GCP/NATS credentials".into(),
            ));
        }
        if credential.provider == Provider::Azure
            && credential.profile.as_deref().is_some_and(|profile| {
                !matches!(profile, "cli" | "managed_identity" | "workload_identity")
            })
        {
            return Err(Error::Config("unknown Azure credential provider".into()));
        }
        if credential.expected_identity.is_some()
            && !matches!(
                credential.provider,
                Provider::Gcp | Provider::Aws | Provider::Azure | Provider::Github
            )
        {
            return Err(Error::Config(
                "identity assertions are unsupported by this credential provider".into(),
            ));
        }
        if matches!(credential.provider, Provider::Nats | Provider::Github)
            && credential.credential_file.is_some()
            && credential.token_env.is_some()
        {
            return Err(Error::Config(
                "select a credential file or token variable".into(),
            ));
        }
        if credential.token_env.as_ref().is_some_and(|name| {
            name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }) {
            return Err(Error::Config("invalid credential variable name".into()));
        }
    }
    Ok(())
}
