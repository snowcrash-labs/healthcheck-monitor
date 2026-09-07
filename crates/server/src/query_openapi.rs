//! OpenAPI schemas are generated from the same contracts consumed by the MCP connector.
use crate::{
    api::App,
    response::{ApiError, json},
};
use axum::{extract::State, response::Response};
use monitor_query::{
    filter::{Deployment, Filter},
    record::Record,
    response::*,
};
use schemars::JsonSchema;
use serde_json::{Value, json as value};
use std::sync::Arc;

#[allow(dead_code)]
#[derive(JsonSchema)]
struct Contracts {
    filter: Filter,
    deployment: Deployment,
    records: Page<Record>,
    scopes: Page<ScopeInfo>,
    summary: Summary,
    resource: ResourceDetail,
    assessment: Assessment,
}
pub fn document() -> Result<Value, ApiError> {
    let root = schemars::schema_for!(Contracts);
    let mut root = serde_json::to_value(root).map_err(|_| ApiError::Unavailable)?;
    fn references(value: &mut Value) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if key == "$ref" {
                        if let Value::String(s) = value {
                            *s = s.replace("#/$defs/", "#/components/schemas/");
                        }
                    } else {
                        references(value);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    references(value);
                }
            }
            _ => {}
        }
    }
    references(&mut root);
    let schemas = root.get("$defs").cloned().ok_or(ApiError::Unavailable)?;
    let mut paths = serde_json::Map::new();
    for (endpoint, request, response, description) in [
        (
            "scopes",
            "Filter",
            "scopes",
            "Discover current and retained monitoring scopes",
        ),
        (
            "summary",
            "Filter",
            "summary",
            "Summarize observed health and collection failures",
        ),
        (
            "findings",
            "Filter",
            "records",
            "Search current and recovered findings overlapping a time window",
        ),
        (
            "diagnostics",
            "Filter",
            "records",
            "Query redacted log signatures and original sampling windows",
        ),
        (
            "resource",
            "Filter",
            "resource",
            "Read current resource metadata and paginated historical evidence",
        ),
        (
            "checks",
            "Filter",
            "records",
            "Query check outcomes and collection coverage",
        ),
        (
            "deployment",
            "Deployment",
            "assessment",
            "Assess fresh evidence after a deployment without starting scans",
        ),
    ] {
        let parameters=schemas.get(request).and_then(|s|s.get("properties")).and_then(Value::as_object).ok_or(ApiError::Unavailable)?
            .iter().map(|(name,schema)|value!({"name":name,"in":"query","required":request=="Deployment"&&name=="deployed_at","schema":schema})).collect::<Vec<_>>();
        let response = root
            .get("properties")
            .and_then(|p| p.get(response))
            .cloned()
            .ok_or(ApiError::Unavailable)?;
        paths.insert(format!("/api/v1/query/{endpoint}"),value!({"get":{
            "summary":description,"operationId":endpoint,"parameters":parameters,
            "responses":{"200":{"description":"Query result with explicit availability","content":{"application/json":{"schema":response}}},
                "400":{"description":"Invalid query or cursor"},"401":{"description":"Google sign-in required"},"403":{"description":"Identity lacks IAP access"},"503":{"description":"Monitor or history unavailable"}},
            "security":[{"googleIap":[]}]
        }}));
    }
    Ok(
        value!({"openapi":"3.1.0","info":{"title":"Soundpatrol monitoring queries","version":"1"},
        "servers":[{"url":"https://health.soundpatrol.com"}],"paths":paths,
        "components":{"schemas":schemas,"securitySchemes":{"googleIap":{"type":"http","scheme":"bearer","bearerFormat":"Google ID token","description":"Obtain a Google ID token using the allowlisted desktop OAuth client. IAP also enforces devops-group membership."}}}}),
    )
}
pub async fn openapi(State(app): State<Arc<App>>) -> Result<Response, ApiError> {
    json(&document()?, app.response_bytes)
}
