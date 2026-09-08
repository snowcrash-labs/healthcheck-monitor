//! Reject credential options that a provider cannot honor.
use super::types::{Config, Credential};
use crate::{error::Error, model::Provider};
pub fn validate(config: &Config) -> Result<(), Error> {
    for (name, credential) in &config.credentials {
        if !super::validate::identifier(name) {
            return Err(Error::Config("invalid credential profile name".into()));
        }
        validate_credential(credential)?;
    }
    Ok(())
}
pub fn validate_credential(credential: &Credential) -> Result<(), Error> {
    if credential
        .profile
        .as_ref()
        .is_some_and(|profile| profile.len() > 512 || profile.chars().any(char::is_control))
        || credential
            .expected_identity
            .as_ref()
            .is_some_and(|identity| identity.len() > 1024 || identity.chars().any(char::is_control))
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
    if let Some(federation) = &credential.google_federation {
        if !matches!(credential.provider, Provider::Aws | Provider::Azure)
            || credential.profile.is_some()
            || federation.subject.is_empty()
            || federation.subject.len() > 32
            || !federation.subject.bytes().all(|b| b.is_ascii_digit())
            || federation.audience.is_empty()
            || federation.audience.len() > 1024
            || federation.audience.chars().any(char::is_whitespace)
            || federation.audience.chars().any(char::is_control)
        {
            return Err(Error::Config("invalid Google federation identity".into()));
        }
        match credential.provider {
            Provider::Aws
                if federation.client_id.is_none()
                    && credential.role_arn.as_deref().is_some_and(role_arn) => {}
            Provider::Azure
                if federation.audience == "api://AzureADTokenExchange"
                    && credential.tenant.as_deref().is_some_and(uuid)
                    && federation.client_id.as_deref().is_some_and(uuid) => {}
            _ => {
                return Err(Error::Config(
                    "incomplete cloud federation configuration".into(),
                ));
            }
        }
    }
    Ok(())
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn role_arn(value: &str) -> bool {
    let parts: Vec<_> = value.splitn(6, ':').collect();
    parts.len() == 6
        && parts[0] == "arn"
        && parts[1] == "aws"
        && parts[2] == "iam"
        && parts[3].is_empty()
        && parts[4].len() == 12
        && parts[4].bytes().all(|b| b.is_ascii_digit())
        && parts[5].starts_with("role/")
        && parts[5].len() > 5
        && value.len() <= 2048
        && parts[5]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/+=,.@_-".contains(&b))
}
