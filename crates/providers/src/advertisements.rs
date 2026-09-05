//! Public web endpoints advertised by control-plane metadata.
use crate::common::Endpoint;
use monitor_integrations::projection::{boolean, text};
use serde_json::Value;
pub fn endpoint(source: &Endpoint, value: &Value) -> Option<String> {
    let family = source.id.split('/').next()?;
    if !matches!(
        family,
        "cloud-run"
            | "load-balancers"
            | "cloudfront"
            | "container-apps"
            | "app-service"
            | "front-door-endpoints"
    ) {
        return None;
    }
    if text(value, &["/Scheme", "/properties/publicNetworkAccess"])
        .is_some_and(|v| matches!(v, "internal" | "Disabled"))
        || boolean(value, &["/properties/configuration/ingress/external"]) == Some(false)
    {
        return None;
    }
    let raw = text(
        value,
        &[
            "/uri",
            "/DNSName",
            "/DomainName",
            "/properties/defaultHostName",
            "/properties/hostName",
            "/properties/configuration/ingress/fqdn",
        ],
    )?;
    let url = if raw.starts_with("https://") {
        url::Url::parse(raw).ok()?
    } else {
        url::Url::parse(&format!("https://{raw}/")).ok()?
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(url.into())
}
