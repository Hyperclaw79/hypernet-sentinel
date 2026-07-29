use crate::{coordinator::StartError, AppState};
use measurement_core::TestSelection;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryArgs {
    /// Time range: 24h, 7d, or 30d.
    pub range: Option<String>,
    /// Maximum result count, from 1 through 1000.
    pub limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct RunArgs {
    /// Measure idle HTTP latency and jitter. Defaults to true.
    pub latency: Option<bool>,
    /// Measure download throughput and loaded latency. Defaults to true.
    pub download: Option<bool>,
    /// Measure upload throughput and loaded latency. Defaults to true.
    pub upload: Option<bool>,
    /// Measure experimental UDP packet loss. Defaults to true.
    pub packet_loss: Option<bool>,
}

#[derive(Clone)]
#[allow(dead_code)] // Read by rmcp's generated ServerHandler implementation.
pub struct SentinelMcp {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl SentinelMcp {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Return application readiness, whether a diagnostic is running, and the latest measurement summary."
    )]
    async fn get_current_status(&self) -> Result<CallToolResult, McpError> {
        let status = self.state.coordinator.status().await;
        match self.state.db.summary().await {
            Ok(summary) => Ok(structured(
                serde_json::json!({"ready":true,"current":status,"summary":summary}),
            )),
            Err(error) => Ok(tool_error(format!("status query failed: {error}"))),
        }
    }

    #[tool(
        description = "Return the most recent completed or failed diagnostic, including structured measurements and error details."
    )]
    async fn get_latest_result(&self) -> Result<CallToolResult, McpError> {
        match self.state.db.latest().await {
            Ok(value) => Ok(structured(serde_json::json!({"result":value}))),
            Err(error) => Ok(tool_error(format!("latest result query failed: {error}"))),
        }
    }

    #[tool(description = "Query diagnostic history over a 24h, 7d, or 30d range.")]
    async fn query_results(
        &self,
        Parameters(args): Parameters<QueryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let range = args.range.unwrap_or_else(|| "24h".into());
        let hours = match range.as_str() {
            "24h" => 24,
            "7d" => 168,
            "30d" => 720,
            _ => return Ok(tool_error("range must be 24h, 7d, or 30d")),
        };
        match self
            .state
            .db
            .results(hours, args.limit.unwrap_or(100).clamp(1, 1_000), false)
            .await
        {
            Ok(results) => Ok(structured(
                serde_json::json!({"range":range,"results":results}),
            )),
            Err(error) => Ok(tool_error(format!("history query failed: {error}"))),
        }
    }

    #[tool(description = "Return the active diagnostic and its current phase, or null when idle.")]
    async fn get_active_test(&self) -> Result<CallToolResult, McpError> {
        Ok(structured(
            serde_json::to_value(self.state.coordinator.status().await).unwrap_or_default(),
        ))
    }

    #[tool(
        description = "Start selected diagnostics using the same execution path as the dashboard. Every option defaults to true. Returns an error if a test is already active."
    )]
    async fn run_test(
        &self,
        Parameters(args): Parameters<RunArgs>,
    ) -> Result<CallToolResult, McpError> {
        let selection = TestSelection {
            latency: args.latency.unwrap_or(true),
            download: args.download.unwrap_or(true),
            upload: args.upload.unwrap_or(true),
            packet_loss: args.packet_loss.unwrap_or(true),
        };
        if selection.is_empty() {
            return Ok(tool_error("select at least one diagnostic"));
        }
        match self
            .state
            .coordinator
            .start_selected(selection, "mcp")
            .await
        {
            Ok(active) => Ok(structured(
                serde_json::json!({"accepted":true,"test":active}),
            )),
            Err(StartError::Conflict) => Ok(tool_error("a diagnostic is already running")),
            Err(StartError::Internal(error)) => {
                Ok(tool_error(format!("could not start diagnostic: {error}")))
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for SentinelMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("hypernet-sentinel", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_instructions("Read Hypernet Sentinel connection history or start one controlled full diagnostic. Measurements currently target Cloudflare infrastructure.")
    }
}

fn structured(value: serde_json::Value) -> CallToolResult {
    CallToolResult::structured(value)
}
fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.into())])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focused_tool_surface_is_registered() {
        let router = SentinelMcp::tool_router();
        for name in [
            "get_current_status",
            "get_latest_result",
            "query_results",
            "get_active_test",
            "run_test",
        ] {
            assert!(router.has_route(name), "missing MCP tool {name}");
        }
        assert_eq!(router.list_all().len(), 5);
    }
}
