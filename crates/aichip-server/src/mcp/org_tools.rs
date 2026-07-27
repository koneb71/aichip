//! Team tools for organization runs, scoped per member.
//!
//! The URL carries both the run and the step, so the server always knows
//! which teammate is speaking without trusting anything the model says.

use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub async fn rpc(
    State(state): State<AppState>,
    Path((run_id, step_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let Some(id) = req.get("id").cloned() else {
        return (StatusCode::ACCEPTED, Json(Value::Null));
    };
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");

    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "aichip", "version": env!("CARGO_PKG_VERSION") }
        }),
        "ping" => json!({}),
        "tools/list" => tools_list(),
        "tools/call" => {
            let name = req.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = req.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            match call_tool(&state, run_id, step_id, name, args).await {
                Ok(payload) => json!({
                    "content": [{ "type": "text", "text": payload.to_string() }]
                }),
                Err(message) => json!({
                    "content": [{ "type": "text", "text": json!({"error": message}).to_string() }],
                    "isError": true
                }),
            }
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

pub fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "post_message",
                "description": "Tell the team what you're doing, what you found, or what you decided. Everyone sees it, including the human watching.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string" },
                        "to": { "type": "string", "description": "optional teammate name to address" }
                    },
                    "required": ["content"]
                }
            },
            {
                "name": "read_messages",
                "description": "Read what the team has said so far on this job.",
                "inputSchema": { "type": "object", "properties": {}, "required": [] }
            },
            {
                "name": "ask_manager",
                "description": "Escalate a decision that isn't yours to make. Blocks until your manager answers. Use sparingly — you get a few of these.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "question": { "type": "string" } },
                    "required": ["question"]
                }
            }
        ]
    })
}

async fn call_tool(
    state: &AppState,
    run_id: Uuid,
    step_id: Uuid,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    let speaker = speaker_of(state, run_id, step_id).await?;

    match name {
        "post_message" => {
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or("content is required")?;
            let to = args.get("to").and_then(Value::as_str);
            state
                .orchestrator
                .post(run_id, Some(step_id), &speaker, to, "message", content)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "posted": true }))
        }
        "read_messages" => {
            let rows = sqlx::query(
                "SELECT from_agent, to_agent, kind, content FROM org_messages
                 WHERE run_id = $1 AND kind <> 'status' ORDER BY seq ASC LIMIT 100",
            )
            .bind(run_id)
            .fetch_all(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({
                "messages": rows.iter().map(|r| json!({
                    "from": r.get::<String, _>("from_agent"),
                    "to": r.get::<Option<String>, _>("to_agent"),
                    "kind": r.get::<String, _>("kind"),
                    "content": r.get::<String, _>("content"),
                })).collect::<Vec<_>>()
            }))
        }
        "ask_manager" => {
            let question = args
                .get("question")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or("question is required")?;
            let answer = state
                .orchestrator
                .consult_manager(run_id, step_id, &speaker, question)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "answer": answer }))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Who owns this step? Taken from the database, never from the caller.
async fn speaker_of(state: &AppState, run_id: Uuid, step_id: Uuid) -> Result<String, String> {
    let row = sqlx::query("SELECT assignee FROM steps WHERE id = $1 AND run_id = $2")
        .bind(step_id)
        .bind(run_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("this step does not belong to that run")?;
    Ok(row
        .get::<Option<String>, _>("assignee")
        .unwrap_or_else(|| "Teammate".to_string()))
}

#[cfg(test)]
mod tests {
    use super::tools_list;

    #[test]
    fn exposes_the_three_team_tools() {
        let tools = tools_list();
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["post_message", "read_messages", "ask_manager"]);
    }
}
