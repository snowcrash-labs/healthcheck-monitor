//! Provider cooldowns release admission capacity and remain inside the operation deadline.
use reqwest::header::HeaderMap;
use std::time::Duration;

/// Azure billing publishes separate entity, tenant, client, and query-capacity cooldowns.
pub(crate) fn from_headers(headers: &HeaderMap) -> Option<Duration> {
    headers
        .iter()
        .filter(|(name, _)| {
            name.as_str() == "retry-after"
                || (name
                    .as_str()
                    .starts_with("x-ms-ratelimit-microsoft.costmanagement-")
                    && name.as_str().ends_with("-retry-after"))
        })
        .filter_map(|(_, value)| value.to_str().ok()?.parse::<u64>().ok())
        .max()
        .map(|seconds| Duration::from_secs(seconds.min(86400)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azure_cooldown_uses_the_longest_applicable_limit() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", "2".parse()?);
        headers.insert(
            "x-ms-ratelimit-microsoft.costmanagement-qpu-retry-after",
            "60".parse()?,
        );
        headers.insert(
            "x-ms-ratelimit-microsoft.costmanagement-tenant-retry-after",
            "30".parse()?,
        );
        headers.insert(
            "x-ms-ratelimit-microsoft.costmanagement-clienttype-retry-after",
            "invalid".parse()?,
        );
        headers.insert("unrelated-retry-after", "900".parse()?);
        assert_eq!(from_headers(&headers), Some(Duration::from_secs(60)));
        Ok(())
    }
}
