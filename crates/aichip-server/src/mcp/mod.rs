//! Hand-rolled MCP-over-HTTP endpoint implementing exactly what Claude
//! Code's `--permission-prompt-tool mcp__aichip__approve` needs: initialize,
//! tools/list, and tools/call for a single `approve` tool. When the engine
//! asks for permission, the call parks in the PermissionBroker until the
//! user answers in the dashboard (timeout → deny).

pub mod chat_tools;

use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};
use uuid::Uuid;

pub fn mcp_router() -> Router<AppState> {
    Router::new()
        .route("/run/{run_id}", post(rpc))
        .route("/chat/{chat_id}", post(chat_tools::rpc))
}

async fn rpc(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(req): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");

    // Notifications (no id) just get acknowledged.
    let Some(id) = id else {
        return (StatusCode::ACCEPTED, Json(Value::Null));
    };

    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "aichip", "version": env!("CARGO_PKG_VERSION") }
        }),
        "ping" => json!({}),
        "tools/list" => json!({
            "tools": [{
                "name": "approve",
                "description": "Ask the aichip dashboard user to approve a tool call.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tool_name": { "type": "string" },
                        "input": { "type": "object" },
                        "tool_use_id": { "type": "string" }
                    },
                    "required": ["tool_name", "input"]
                }
            }]
        }),
        "tools/call" => {
            let args = req.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            let tool_name = args
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let input = args.get("input").cloned().unwrap_or(json!({}));

            let allowed = state
                .permissions
                .request(run_id, tool_name, input.clone())
                .await;

            // The permission-prompt-tool contract: content[0].text is a
            // JSON-encoded {behavior, updatedInput|message}.
            let payload = if allowed {
                json!({ "behavior": "allow", "updatedInput": input })
            } else {
                json!({ "behavior": "deny", "message": "denied by aichip user" })
            };
            json!({
                "content": [{ "type": "text", "text": payload.to_string() }]
            })
        }
        _ => {
            return (
                StatusCode::OK,
                Json(json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("method not found: {method}") }
                })),
            );
        }
    };

    (
        StatusCode::OK,
        Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
    )
}
