//! Provider-specific cursors preserve completeness without redirecting credentials.
use crate::common::Endpoint;
use monitor_core::{config::resolve::Job, model::Provider};
use monitor_integrations::{
    projection::{boolean, text},
    transport::Error,
};
use serde_json::Value;
pub fn advance(
    endpoint: &mut Endpoint,
    job: &Job,
    payload: &Value,
    previous: &str,
    page: usize,
) -> Result<Option<String>, Error> {
    if endpoint
        .aws
        .as_ref()
        .is_some_and(|(service, _, _)| service == "route53")
        && endpoint.url.contains("/rrset")
        && boolean(payload, &["/IsTruncated"]) == Some(true)
    {
        let name = text(payload, &["/NextRecordName"]).ok_or(Error::Malformed)?;
        let kind = text(payload, &["/NextRecordType"]).ok_or(Error::Malformed)?;
        let identifier = text(payload, &["/NextRecordIdentifier"]).unwrap_or("");
        let key = serde_json::to_string(&(name, kind, identifier)).map_err(|_| Error::Malformed)?;
        validate(&key, previous, page, job)?;
        query(
            endpoint,
            &[("name", name), ("type", kind), ("identifier", identifier)],
        )?;
        return Ok(Some(key));
    }
    let mut cursor = None;
    for path in [
        "/nextPageToken",
        "/nextToken",
        "/NextToken",
        "/nextLink",
        "/NextMarker",
        "/ContinuationToken",
        "/$skipToken",
        "/LastEvaluatedTableName",
        "/DescribeDBInstancesResult/Marker",
        "/DescribeDBClustersResult/Marker",
        "/DescribeCacheClustersResult/Marker",
        "/DescribeReplicationGroupsResult/Marker",
        "/DescribeAutoScalingGroupsResult/NextToken",
        "/DescribeLoadBalancersResult/NextMarker",
        "/DescribeTargetGroupsResult/NextMarker",
        "/ListTopicsResult/NextToken",
    ] {
        match payload.pointer(path) {
            None | Some(Value::Null) => {}
            Some(Value::String(token)) if token.is_empty() => {}
            Some(Value::String(token)) => {
                cursor = Some((path, token));
                break;
            }
            Some(_) => return Err(Error::Malformed),
        }
    }
    let Some((path, token)) = cursor else {
        return if boolean(payload, &["/IsTruncated", "/Truncated"]) == Some(true) {
            Err(Error::Malformed)
        } else {
            Ok(None)
        };
    };
    validate(token, previous, page, job)?;
    if path == "/nextLink" {
        let next = url::Url::parse(token).map_err(|_| Error::Malformed)?;
        let old = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed)?;
        if !next.username().is_empty()
            || next.password().is_some()
            || next.fragment().is_some()
            || next.origin() != old.origin()
            || !(next
                .path()
                .starts_with(&format!("/subscriptions/{}/", job.target.scope))
                || old
                    .host_str()
                    .is_some_and(|host| host.ends_with(".vault.azure.net"))
                    && next.path() == old.path())
        {
            return Err(Error::Forbidden);
        }
        endpoint.url = next.into();
    } else {
        let field = field(endpoint, job, path);
        if let Some(body) = endpoint.body.as_mut() {
            if path == "/$skipToken" {
                body["options"]["$skipToken"] = Value::String(token.clone());
            } else {
                body[field] = Value::String(token.clone());
            }
        } else {
            query(endpoint, &[(field, token)])?;
        }
    }
    Ok(Some(token.clone()))
}
fn validate(token: &str, previous: &str, page: usize, job: &Job) -> Result<(), Error> {
    if token.len() > 4096 || token == previous || page >= job.settings.max_pages {
        Err(Error::Limit)
    } else {
        Ok(())
    }
}
fn field<'a>(endpoint: &Endpoint, job: &Job, path: &'a str) -> &'a str {
    if job.target.provider == Provider::Gcp {
        return "pageToken";
    }
    if let Some((service, _, target)) = &endpoint.aws {
        if path == "/LastEvaluatedTableName" {
            return "ExclusiveStartTableName";
        }
        if target.starts_with("query:") {
            return if matches!(
                service.as_str(),
                "rds" | "elasticache" | "elasticloadbalancing"
            ) {
                "Marker"
            } else {
                "NextToken"
            };
        }
        if service == "kms" {
            return "Marker";
        }
        if endpoint.body.is_none() {
            return match service.as_str() {
                "eks" | "backup" => "nextToken",
                "route53" => "marker",
                "lambda" | "cloudfront" => "Marker",
                "s3" => "continuation-token",
                _ => "NextToken",
            };
        }
    }
    path.trim_start_matches('/')
}
fn query(endpoint: &mut Endpoint, replacement: &[(&str, &str)]) -> Result<(), Error> {
    let mut url = url::Url::parse(&endpoint.url).map_err(|_| Error::Malformed)?;
    let pairs: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| !replacement.iter().any(|(field, _)| key == field))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(pairs)
        .extend_pairs(
            replacement
                .iter()
                .filter(|(_, value)| !value.is_empty())
                .copied(),
        );
    endpoint.url = url.into();
    Ok(())
}
