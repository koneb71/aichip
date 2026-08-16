//! Hand-rolled MCP-over-HTTP endpoint implementing exactly what Claude
//! Code's `--permission-prompt-tool mcp__aichip__approve` needs: initialize,
//! tools/list, and tools/call for a single `approve` tool. When the engine
//! asks for permission, the call parks in the PermissionBroker until the
//! user answers in the dashboard.

pub mod chat_tools;
pub mod org_tools;

use crate::AppState;
use aichip_core::runs::permissions::Decision;
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
        .route("/org/{run_id}/{step_id}", post(org_tools::rpc))
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
            let args = req
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or(json!({}));
            let tool_name = args
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let input = args.get("input").cloned().unwrap_or(json!({}));

            let decision = state
                .permissions
                .request(run_id, tool_name, input.clone())
                .await;

            // The permission-prompt-tool contract: content[0].text is a
            // JSON-encoded {behavior, updatedInput|message}.
            let payload = match &decision {
                Decision::Allowed => json!({ "behavior": "allow", "updatedInput": input }),
                other => json!({ "behavior": "deny", "message": refusal(other) }),
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

/// What to say when the answer is not "allow".
///
/// The wire protocol has only allow and deny, so everything here travels as a
/// denial — but only one of these is a person refusing. An engine told it was
/// refused works around the refusal and spends real money doing it, so the
/// message is where the difference has to survive. `Unanswered` and `RunGone`
/// both say plainly that the run is over, because the broker has already
/// stopped it and any further work would be thrown away.
fn refusal(decision: &Decision) -> &'static str {
    match decision {
        // Unreachable: the caller matches Allowed before getting here.
        Decision::Allowed => "allowed",
        Decision::Denied => "denied by the aichip user",
        Decision::Unanswered { .. } => {
            "nobody answered this request, so aichip stopped the run. \
             This is not a refusal — no one saw it. Do not work around it; stop here."
        }
        Decision::RunGone => "this run was cancelled while the request was outstanding; stop here.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_engine_is_never_told_a_person_refused_something_nobody_saw() {
        let unanswered = refusal(&Decision::Unanswered {
            waited: Duration::from_secs(86_400),
        });
        // The exact sentence this replaced was "denied by aichip user", which
        // described a decision that had not happened.
        assert!(!unanswered.contains("denied"), "{unanswered}");
        assert!(unanswered.contains("not a refusal"), "{unanswered}");

        assert!(refusal(&Decision::Denied).contains("denied"));
        assert!(refusal(&Decision::RunGone).contains("cancelled"));
    }

    #[test]
    fn every_ending_tells_the_engine_something_different() {
        let all = [
            refusal(&Decision::Denied),
            refusal(&Decision::Unanswered {
                waited: Duration::from_secs(1),
            }),
            refusal(&Decision::RunGone),
        ];
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "two endings read the same");
    }
}
