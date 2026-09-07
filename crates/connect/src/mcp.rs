//! Read-only MCP tools forward typed requests to the protected monitoring query API.
use crate::{client::Client, error::Error};
use monitor_query::{
    filter::{Deployment, Filter},
    record::Record,
    response::{Assessment, Page, ScopeInfo, Summary},
};
use rmcp::{
    Json, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

#[derive(Clone)]
pub struct Connector {
    client: Client,
    tool_router: ToolRouter<Self>,
}
impl Connector {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}
#[tool_router]
impl Connector {
    /// Discover target names, native cloud scopes, and configured monitoring categories.
    #[tool(annotations(
        read_only_hint = true,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn list_scopes(
        &self,
        Parameters(filter): Parameters<Filter>,
    ) -> Result<Json<Page<ScopeInfo>>, String> {
        self.client
            .get("scopes", &filter)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }
    /// Summarize observed problems in an explicit or relative period. Missing coverage is not healthy silence.
    #[tool(annotations(
        read_only_hint = true,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn get_health_summary(
        &self,
        Parameters(filter): Parameters<Filter>,
    ) -> Result<Json<Summary>, String> {
        self.client
            .get("summary", &filter)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }
    /// Search current and recovered finding episodes overlapping the requested period. Follow next_cursor for further pages.
    #[tool(annotations(
        read_only_hint = true,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn search_findings(
        &self,
        Parameters(filter): Parameters<Filter>,
    ) -> Result<Json<Page<Record>>, String> {
        self.client
            .get("findings", &filter)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }
    /// Query grouped redacted log signatures and their original sampling windows. Counts are samples, not complete error rates.
    #[tool(annotations(
        read_only_hint = true,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn search_diagnostics(
        &self,
        Parameters(filter): Parameters<Filter>,
    ) -> Result<Json<Page<Record>>, String> {
        self.client
            .get("diagnostics", &filter)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }
    /// Retrieve resource evidence and cloud-console links. The resource filter is required.
    #[tool(annotations(
        read_only_hint = true,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn get_resource(
        &self,
        Parameters(filter): Parameters<Filter>,
    ) -> Result<Json<monitor_query::response::ResourceDetail>, String> {
        self.client
            .get("resource", &filter)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }
    /// Inspect check outcomes, source timestamps, collection failures, and incomplete coverage.
    #[tool(annotations(
        read_only_hint = true,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn get_checks(
        &self,
        Parameters(filter): Parameters<Filter>,
    ) -> Result<Json<Page<Record>>, String> {
        self.client
            .get("checks", &filter)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }
    /// Assess evidence collected after deployment against an equal preceding baseline. This does not trigger scans. Missing fresh evidence yields pending or incomplete.
    #[tool(annotations(
        read_only_hint = true,
        destructive_hint = false,
        idempotent_hint = true,
        open_world_hint = false
    ))]
    async fn assess_deployment(
        &self,
        Parameters(query): Parameters<Deployment>,
    ) -> Result<Json<Assessment>, String> {
        self.client
            .get("deployment", &query)
            .await
            .map(Json)
            .map_err(|e| e.to_string())
    }
}
#[tool_handler(router=self.tool_router)]
impl ServerHandler for Connector {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Query the continuous Soundpatrol monitor. Discover scopes first. Keep health, collection coverage, freshness, and persistence gaps separate. Follow pagination cursors with unchanged filters. Short deployment windows require post-deployment evidence; never treat older scans or missing logs as a pass. This connector cannot run scans, consume messages, or change infrastructure. Google sign-in is a separate operator action: healthcheck-connect login."
        )
    }
}
pub async fn serve(client: Client) -> Result<(), Error> {
    let service = Connector::new(client)
        .serve((
            crate::input::Limited::new(tokio::io::stdin()),
            tokio::io::stdout(),
        ))
        .await
        .map_err(|_| Error::Transport)?;
    service.waiting().await.map_err(|_| Error::Transport)?;
    Ok(())
}
