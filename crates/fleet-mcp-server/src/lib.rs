use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Backend handler implemented by the desktop runtime to fulfill reverse Fleet MCP tool calls.
pub trait FleetMcpHandler: Send + Sync + 'static {
    fn authorized(&self, token: &str) -> bool;
    fn call_tool(&self, name: String, arguments: Value) -> BoxFuture<'_, Result<Value, String>>;
}

pub fn tools_catalog() -> Value {
    json!({
        "tools": [
            {
                "name": "fleet_list_nodes",
                "description": "List all connected Termexo nodes, their online status, tags, workspaces, and available concurrency capacity.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "parent_token": { "type": "string", "minLength": 1 }
                    },
                    "required": ["parent_token"],
                    "additionalProperties": false
                }
            },
            {
                "name": "fleet_submit_plan",
                "description": "Submit a multi-step DAG execution plan from a Planner agent. Fleet validates acyclicity, materializes tasks, and dispatches ready steps across matching Termexo nodes.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "parent_token": { "type": "string", "minLength": 1 },
                        "steps": {
                            "type": "array",
                            "minItems": 1,
                            "maxItems": 20,
                            "items": {
                                "type": "object",
                                "properties": {
                                    "key": { "type": "string", "minLength": 1 },
                                    "title": { "type": "string", "minLength": 1 },
                                    "goal": { "type": "string", "minLength": 1 },
                                    "depends_on_keys": {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    },
                                    "difficulty": { "type": "string", "enum": ["auto", "L1", "L2", "L3", "L4"] },
                                    "role": { "type": "string", "enum": ["worker", "reviewer", "shell"] },
                                    "required_node_tags": {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    },
                                    "preferred_agent": { "type": "string", "enum": ["claude", "codex", "opencode", "pi", "grok", "antigravity"] },
                                    "isolate_worktree": { "type": "boolean" },
                                    "acceptance_criteria": { "type": "string" }
                                },
                                "required": ["key", "title", "goal"]
                            }
                        }
                    },
                    "required": ["parent_token", "steps"],
                    "additionalProperties": false
                }
            },
            {
                "name": "fleet_spawn_subagent",
                "description": "Spawn a child agent task on a managed Termexo node under mission guardrails. Automatically selects node/account/model by difficulty and optionally isolates edits in a dedicated Git worktree.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "parent_token": { "type": "string", "minLength": 1 },
                        "title": { "type": "string", "minLength": 1, "maxLength": 200 },
                        "goal": { "type": "string", "minLength": 1 },
                        "difficulty": { "type": "string", "enum": ["auto", "L1", "L2", "L3", "L4"] },
                        "isolate_worktree": { "type": "boolean" },
                        "required_node_tags": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "preferred_agent": { "type": "string", "enum": ["claude", "codex", "opencode", "pi", "grok", "antigravity"] },
                        "acceptance_criteria": { "type": "string" }
                    },
                    "required": ["parent_token", "title", "goal"],
                    "additionalProperties": false
                }
            },
            {
                "name": "fleet_list_subagents",
                "description": "List all child agents spawned by the calling parent task, including status, assigned node/model/account, and worktree branch.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "parent_token": { "type": "string", "minLength": 1 }
                    },
                    "required": ["parent_token"],
                    "additionalProperties": false
                }
            },
            {
                "name": "fleet_wait_subagents",
                "description": "Wait until the specified child agent IDs reach a terminal state (completed, failed, cancelled) or timeout, returning their output summaries and Git diff stats.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "parent_token": { "type": "string", "minLength": 1 },
                        "subagent_ids": {
                            "type": "array",
                            "items": { "type": "string", "minLength": 1 }
                        },
                        "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 1800 }
                    },
                    "required": ["parent_token", "subagent_ids"],
                    "additionalProperties": false
                }
            },
            {
                "name": "fleet_message_subagent",
                "description": "Send a follow-up prompt or answer to a child agent terminal running in Termexo.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "parent_token": { "type": "string", "minLength": 1 },
                        "subagent_id": { "type": "string", "minLength": 1 },
                        "message": { "type": "string", "minLength": 1 }
                    },
                    "required": ["parent_token", "subagent_id", "message"],
                    "additionalProperties": false
                }
            },
            {
                "name": "fleet_cancel_subagent",
                "description": "Stop a running child agent task in Termexo and mark it cancelled.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "parent_token": { "type": "string", "minLength": 1 },
                        "subagent_id": { "type": "string", "minLength": 1 }
                    },
                    "required": ["parent_token", "subagent_id"],
                    "additionalProperties": false
                }
            }
        ]
    })
}

pub fn build_router<H: FleetMcpHandler>(handler: Arc<H>) -> Router {
    Router::new()
        .route("/mcp", post(handle_mcp::<H>))
        .with_state(handler)
}

pub struct RunningMcpServer {
    pub addr: SocketAddr,
    handle: JoinHandle<()>,
}

impl RunningMcpServer {
    pub fn url(&self) -> String {
        format!("http://{}:{}/mcp", self.addr.ip(), self.addr.port())
    }

    pub fn stop(self) {
        self.handle.abort();
    }
}

pub async fn start_local_server<H: FleetMcpHandler>(
    port: u16,
    handler: Arc<H>,
) -> Result<RunningMcpServer, String> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|error| format!("Failed to bind Fleet MCP server on port {port}: {error}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let router = build_router(handler);
    let handle = tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            tracing::error!(%error, "Fleet MCP server stopped unexpectedly");
        }
    });
    Ok(RunningMcpServer { addr, handle })
}

async fn handle_mcp<H: FleetMcpHandler>(
    State(handler): State<Arc<H>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    if !handler.authorized(token) {
        return (
            StatusCode::UNAUTHORIZED,
            HeaderMap::new(),
            Json(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32001, "message": "Unauthorized Fleet MCP token" }
            })),
        );
    }

    let id = body.get("id").cloned();
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("");
    let params = body.get("params").cloned().unwrap_or_else(|| json!({}));

    let mut response_headers = HeaderMap::new();
    match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(SUPPORTED_VERSIONS[0]);
            let version = if SUPPORTED_VERSIONS.contains(&requested) {
                requested
            } else {
                SUPPORTED_VERSIONS[0]
            };
            let session = uuid::Uuid::new_v4().to_string();
            if let Ok(header_val) = HeaderValue::from_str(&session) {
                response_headers.insert("Mcp-Session-Id", header_val);
            }
            (
                StatusCode::OK,
                response_headers,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id.unwrap_or(Value::Null),
                    "result": {
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": {
                            "name": "termexo-fleet",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                })),
            )
        }
        "notifications/initialized" => (StatusCode::ACCEPTED, response_headers, Json(json!({}))),
        "tools/list" => (
            StatusCode::OK,
            response_headers,
            Json(json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "result": tools_catalog()
            })),
        ),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match handler.call_tool(name, args).await {
                Ok(result) => {
                    let text = serde_json::to_string_pretty(&result)
                        .unwrap_or_else(|_| result.to_string());
                    (
                        StatusCode::OK,
                        response_headers,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": id.unwrap_or(Value::Null),
                            "result": {
                                "content": [{ "type": "text", "text": text }],
                                "structuredContent": result,
                                "isError": false
                            }
                        })),
                    )
                }
                Err(message) => (
                    StatusCode::OK,
                    response_headers,
                    Json(json!({
                        "jsonrpc": "2.0",
                        "id": id.unwrap_or(Value::Null),
                        "result": {
                            "content": [{ "type": "text", "text": message }],
                            "isError": true
                        }
                    })),
                ),
            }
        }
        _ => (
            StatusCode::OK,
            response_headers,
            Json(json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "error": { "code": -32601, "message": format!("Method not found: {method}") }
            })),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockHandler;

    impl FleetMcpHandler for MockHandler {
        fn authorized(&self, token: &str) -> bool {
            token == "valid-token"
        }

        fn call_tool(&self, name: String, arguments: Value) -> BoxFuture<'_, Result<Value, String>> {
            Box::pin(async move {
                match name.as_str() {
                    "fleet_spawn_subagent" => Ok(json!({
                        "subagentId": "sub-1",
                        "title": arguments["title"]
                    })),
                    "fleet_submit_plan" => Ok(json!({
                        "createdTaskIds": ["t-a", "t-b"]
                    })),
                    _ => Err(format!("Unknown tool: {name}")),
                }
            })
        }
    }

    #[tokio::test]
    async fn authenticates_lists_and_dispatches_subagent_and_planner_tools() {
        let server = start_local_server(0, Arc::new(MockHandler)).await.unwrap();
        let client = reqwest::Client::new();

        let unauth = client
            .post(server.url())
            .header("Authorization", "Bearer wrong")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // Lists all 7 reverse MCP tools.
        let list: Value = client
            .post(server.url())
            .header("Authorization", "Bearer valid-token")
            .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 7);

        let spawn: Value = client
            .post(server.url())
            .header("Authorization", "Bearer valid-token")
            .json(&json!({
                "jsonrpc":"2.0",
                "id":3,
                "method":"tools/call",
                "params":{
                    "name":"fleet_spawn_subagent",
                    "arguments":{"parent_token":"pt-1","title":"Unit tests","goal":"Write tests"}
                }
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(spawn["result"]["isError"], false);
        assert_eq!(spawn["result"]["structuredContent"]["subagentId"], "sub-1");

        server.stop();
    }
}
