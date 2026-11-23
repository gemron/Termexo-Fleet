use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use fleet_protocol::{
    NodeProbeResult, TermexoSettingsSnapshot, TermexoTerminalSummary, TermexoWorkspaceSummary,
};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use tokio::sync::RwLock;

const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("MCP JSON-RPC error ({code}): {message}")]
    Rpc { code: i64, message: String },
    #[error("MCP tool call failed: {0}")]
    ToolError(String),
    #[error("Unexpected MCP payload: {0}")]
    InvalidPayload(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSignal {
    RateLimited,
    AuthExpired,
    ApiError,
    WaitingApproval,
    WaitingInput,
    Normal,
}

/// Classifies plain terminal output tails into actionable orchestration signals.
pub fn classify_terminal_output(text: &str) -> TerminalSignal {
    let lower = text.to_ascii_lowercase();
    if lower.contains("login expired")
        || lower.contains("authentication failed")
        || lower.contains("invalid api key")
        || lower.contains("unauthorized")
        || lower.contains("please run /login")
    {
        return TerminalSignal::AuthExpired;
    }
    if lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("status 429")
        || lower.contains("http 429")
        || lower.contains("overloaded_error")
        || lower.contains("quota exceeded")
    {
        return TerminalSignal::RateLimited;
    }
    if lower.contains("api error: 4")
        || lower.contains("api error: 5")
        || lower.contains("model not exist")
        || lower.contains("insufficient balance")
        || lower.contains("insufficient_quota")
        || lower.contains("not_found_error")
        || lower.contains("bad_request_error")
    {
        return TerminalSignal::ApiError;
    }
    let tail: String = lower
        .lines()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    if tail.contains("allow once")
        || tail.contains("approve")
        || tail.contains("permission")
        || tail.contains("do you want to proceed")
        || tail.contains("[y/n]")
    {
        return TerminalSignal::WaitingApproval;
    }
    TerminalSignal::Normal
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTermexoTaskRequest {
    pub workspace_id: String,
    pub project_id: String,
    pub title: String,
    pub agent_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance_criteria: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_profile_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoTaskRecord {
    pub id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub title: String,
    pub stage: String,
    pub execution_state: String,
    #[serde(default)]
    pub prompt_delivery_state: String,
    pub agent_type: String,
    #[serde(default)]
    pub model_name: String,
    pub account_profile_id: Option<String>,
    pub terminal_id: Option<String>,
    pub last_terminal_status: Option<String>,
    pub output_tail: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoProjectRecord {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoTaskBoardSnapshot {
    pub workspace_id: String,
    #[serde(default)]
    pub projects: Vec<TermexoProjectRecord>,
    #[serde(default)]
    pub tasks: Vec<TermexoTaskRecord>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalReadResponse {
    pub data: String,
    #[serde(default)]
    pub sequence: u64,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Clone, Debug, Default)]
struct NodeCapabilities {
    task_create_supports_account: bool,
    task_create_supports_mcp_profiles: bool,
    discovered: bool,
}

pub struct TermexoMcpClient {
    endpoint: String,
    token: String,
    http: reqwest::Client,
    session_id: RwLock<Option<String>>,
    capabilities: RwLock<NodeCapabilities>,
    next_id: AtomicU64,
}

impl TermexoMcpClient {
    pub fn new(endpoint: impl Into<String>, token: impl Into<String>) -> Result<Self, ConnectorError> {
        Self::new_with_options(endpoint, token, false)
    }

    pub fn new_with_options(
        endpoint: impl Into<String>,
        token: impl Into<String>,
        allow_insecure_tls: bool,
    ) -> Result<Self, ConnectorError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(70))
            .danger_accept_invalid_certs(allow_insecure_tls)
            .build()?;
        Ok(Self {
            endpoint: endpoint.into(),
            token: token.into(),
            http,
            session_id: RwLock::new(None),
            capabilities: RwLock::new(NodeCapabilities::default()),
            next_id: AtomicU64::new(1),
        })
    }

    /// Probes reachability, round-trip latency, capabilities, and workspace/terminal counts.
    pub async fn probe(&self) -> NodeProbeResult {
        let started = tokio::time::Instant::now();
        if let Err(err) = self.initialize().await {
            return NodeProbeResult {
                reachable: false,
                latency_ms: started.elapsed().as_millis() as u64,
                workspace_count: 0,
                terminal_count: 0,
                supports_account_routing: false,
                supports_mcp_profiles: false,
                error: Some(err.to_string()),
            };
        }
        self.discover_capabilities().await;
        let caps = self.capabilities.read().await.clone();
        let workspaces = match self.workspace_list().await {
            Ok(ws) => ws,
            Err(err) => {
                return NodeProbeResult {
                    reachable: false,
                    latency_ms: started.elapsed().as_millis() as u64,
                    workspace_count: 0,
                    terminal_count: 0,
                    supports_account_routing: caps.task_create_supports_account,
                    supports_mcp_profiles: caps.task_create_supports_mcp_profiles,
                    error: Some(err.to_string()),
                };
            }
        };
        let terminals = self.terminal_list(None).await.unwrap_or_default();
        NodeProbeResult {
            reachable: true,
            latency_ms: started.elapsed().as_millis() as u64,
            workspace_count: workspaces.len(),
            terminal_count: terminals.len(),
            supports_account_routing: caps.task_create_supports_account,
            supports_mcp_profiles: caps.task_create_supports_mcp_profiles,
            error: None,
        }
    }

    pub async fn wechat_notify(
        &self,
        workspace_id: &str,
        event_key: &str,
        kind: &str,
        terminal_id: Option<&str>,
        task_title: Option<&str>,
    ) -> Result<Value, ConnectorError> {
        let mut args = json!({
            "workspaceId": workspace_id,
            "eventKey": event_key,
            "kind": kind
        });
        if let Some(tid) = terminal_id {
            args["terminalId"] = json!(tid);
        }
        if let Some(title) = task_title {
            args["taskTitle"] = json!(title);
        }
        self.call_tool("wechat_notify", args).await
    }

    pub async fn initialize(&self) -> Result<Value, ConnectorError> {
        let result = self
            .rpc(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "termexo-fleet",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .await?;
        let _ = self.notify("notifications/initialized", json!({})).await;
        Ok(result)
    }

    pub async fn list_tools(&self) -> Result<Vec<String>, ConnectorError> {
        self.ensure_initialized().await?;
        let result = self.rpc("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| ConnectorError::InvalidPayload("missing tools array".into()))?;
        Ok(tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect())
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value, ConnectorError> {
        self.ensure_initialized().await?;
        let result = self
            .rpc(
                "tools/call",
                json!({
                    "name": name,
                    "arguments": arguments
                }),
            )
            .await?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            let message = extract_tool_text(&result).unwrap_or_else(|| result.to_string());
            return Err(ConnectorError::ToolError(message));
        }
        if let Some(structured) = result.get("structuredContent") {
            return Ok(structured.clone());
        }
        if let Some(text) = extract_tool_text(&result) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                return Ok(parsed);
            }
            return Ok(Value::String(text));
        }
        Ok(result)
    }

    pub async fn workspace_list(&self) -> Result<Vec<TermexoWorkspaceSummary>, ConnectorError> {
        let value = self.call_tool("workspace_list", json!({})).await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))
    }

    pub async fn terminal_list(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<Vec<TermexoTerminalSummary>, ConnectorError> {
        let args = match workspace_id {
            Some(id) => json!({ "workspaceId": id }),
            None => json!({}),
        };
        let value = self.call_tool("terminal_list", args).await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))
    }

    pub async fn terminal_read(
        &self,
        terminal_id: &str,
        max_chars: Option<u32>,
    ) -> Result<TerminalReadResponse, ConnectorError> {
        let mut args = json!({ "terminalId": terminal_id, "format": "text" });
        if let Some(max) = max_chars {
            args["maxChars"] = json!(max);
        }
        let value = self.call_tool("terminal_read", args).await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))
    }

    pub async fn terminal_write(
        &self,
        terminal_id: &str,
        data: &str,
        submit: bool,
    ) -> Result<Value, ConnectorError> {
        self.call_tool(
            "terminal_write",
            json!({ "terminalId": terminal_id, "data": data, "submit": submit }),
        )
        .await
    }

    pub async fn settings_get(&self) -> Result<TermexoSettingsSnapshot, ConnectorError> {
        let value = self.call_tool("settings_get", json!({})).await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))
    }

    pub async fn task_list(&self, workspace_id: &str) -> Result<TermexoTaskBoardSnapshot, ConnectorError> {
        let value = self
            .call_tool("task_list", json!({ "workspaceId": workspace_id }))
            .await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))
    }

    pub async fn task_create(
        &self,
        request: &CreateTermexoTaskRequest,
    ) -> Result<TermexoTaskRecord, ConnectorError> {
        self.discover_capabilities().await;
        let caps = self.capabilities.read().await.clone();
        let mut args =
            serde_json::to_value(request).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))?;
        if let Some(obj) = args.as_object_mut() {
            if caps.discovered && !caps.task_create_supports_account {
                obj.remove("accountProfileId");
            }
            if caps.discovered && !caps.task_create_supports_mcp_profiles {
                obj.remove("mcpProfileIds");
            }
        }
        let value = self.call_tool("task_create", args).await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))
    }

    async fn discover_capabilities(&self) {
        if self.capabilities.read().await.discovered {
            return;
        }
        if self.ensure_initialized().await.is_err() {
            return;
        }
        let Ok(result) = self.rpc("tools/list", json!({})).await else {
            return;
        };
        let Some(tools) = result.get("tools").and_then(Value::as_array) else {
            return;
        };
        if let Some(task_create) = tools.iter().find(|t| t.get("name").and_then(Value::as_str) == Some("task_create")) {
            let props = task_create
                .get("inputSchema")
                .and_then(|s| s.get("properties"))
                .and_then(Value::as_object);
            let supports_account = props.is_some_and(|p| p.contains_key("accountProfileId"));
            let supports_mcp = props.is_some_and(|p| p.contains_key("mcpProfileIds"));
            *self.capabilities.write().await = NodeCapabilities {
                task_create_supports_account: supports_account,
                task_create_supports_mcp_profiles: supports_mcp,
                discovered: true,
            };
        }
    }

    pub async fn task_execute(&self, task_id: &str) -> Result<TermexoTaskRecord, ConnectorError> {
        let value = self
            .call_tool("task_execute", json!({ "taskId": task_id }))
            .await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))
    }

    pub async fn task_stop(&self, task_id: &str) -> Result<TermexoTaskRecord, ConnectorError> {
        let value = self
            .call_tool("task_stop", json!({ "taskId": task_id }))
            .await?;
        serde_json::from_value(value).map_err(|e| ConnectorError::InvalidPayload(e.to_string()))
    }

    async fn ensure_initialized(&self) -> Result<(), ConnectorError> {
        if self.session_id.read().await.is_none() {
            self.initialize().await?;
        }
        Ok(())
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), ConnectorError> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        let mut req = self
            .http
            .post(&self.endpoint)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .json(&payload);
        if let Some(session) = self.session_id.read().await.as_deref() {
            req = req.header("Mcp-Session-Id", session);
        }
        let _ = req.send().await?;
        Ok(())
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, ConnectorError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        let mut req = self
            .http
            .post(&self.endpoint)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .json(&payload);
        if let Some(session) = self.session_id.read().await.as_deref() {
            req = req.header("Mcp-Session-Id", session);
        }
        let response = req.send().await?;
        if let Some(session) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            *self.session_id.write().await = Some(session.to_owned());
        }
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(ConnectorError::ToolError(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body.trim()
            )));
        }
        let message = parse_mcp_response_body(&body)?;
        if let Some(error) = message.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(-32000);
            let msg = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Unknown MCP error")
                .to_owned();
            return Err(ConnectorError::Rpc { code, message: msg });
        }
        message
            .get("result")
            .cloned()
            .ok_or_else(|| ConnectorError::InvalidPayload("missing JSON-RPC result".into()))
    }
}

fn parse_mcp_response_body(body: &str) -> Result<Value, ConnectorError> {
    let trimmed = body.trim();
    if trimmed.starts_with('{') {
        return serde_json::from_str(trimmed)
            .map_err(|e| ConnectorError::InvalidPayload(e.to_string()));
    }
    // Streamable HTTP SSE format: extract the last `data:` JSON line.
    let mut last_json: Option<Value> = None;
    for line in trimmed.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            let candidate = rest.trim();
            if candidate.starts_with('{') {
                if let Ok(parsed) = serde_json::from_str::<Value>(candidate) {
                    last_json = Some(parsed);
                }
            }
        }
    }
    last_json.ok_or_else(|| ConnectorError::InvalidPayload(trimmed.to_owned()))
}

fn extract_tool_text(result: &Value) -> Option<String> {
    let items = result.get("content")?.as_array()?;
    let mut parts = Vec::new();
    for item in items {
        if item.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                parts.push(text);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_json_and_sse_mcp_responses() {
        let json_body = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        assert_eq!(parse_mcp_response_body(json_body).unwrap()["id"], 1);

        let sse_body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(parse_mcp_response_body(sse_body).unwrap()["result"]["ok"], true);
    }

    #[test]
    fn classifies_terminal_output_signals() {
        assert_eq!(
            classify_terminal_output("Error: Login expired, please run /login"),
            TerminalSignal::AuthExpired
        );
        assert_eq!(
            classify_terminal_output("HTTP 429: Too Many Requests - rate limit exceeded"),
            TerminalSignal::RateLimited
        );
        assert_eq!(
            classify_terminal_output("Run command rm -rf dist?\nDo you want to proceed [y/n]"),
            TerminalSignal::WaitingApproval
        );
        assert_eq!(
            classify_terminal_output("API Error: 422 Model Not Exist: DeepSeek-V4-Flash"),
            TerminalSignal::ApiError
        );
        assert_eq!(
            classify_terminal_output("All 42 unit tests passed."),
            TerminalSignal::Normal
        );
    }
}
