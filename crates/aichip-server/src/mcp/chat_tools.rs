//! Workspace tools MCP endpoint for chat runs: the project assistant calls
//! these to create/start/inspect tasks. Every tool resolves through the
//! chat's own project, so a chat can never touch another project's data.

use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub async fn rpc(
    State(state): State<AppState>,
    Path(chat_id): Path<Uuid>,
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
            let name = req
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let args = req.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            match call_tool(&state, chat_id, name, args).await {
                Ok(payload) => json!({
                    "content": [{ "type": "text", "text": payload.to_string() }]
                }),
                Err(msg) => json!({
                    "content": [{ "type": "text", "text": json!({"error": msg}).to_string() }],
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
    let obj = |props: Value, required: Vec<&str>| {
        json!({ "type": "object", "properties": props, "required": required })
    };
    json!({
        "tools": [
            {
                "name": "create_task",
                "description": "Create a coding task on the project board. It runs in an isolated git worktree and its result appears for user review. Set start=true to launch immediately.",
                "inputSchema": obj(json!({
                    "title": { "type": "string" },
                    "prompt": { "type": "string", "description": "full instructions for the coding agent" },
                    "agent_name": { "type": "string", "description": "optional: bind a named agent from the library, spelled exactly as list_agents reports it. An unknown name is rejected. Omit it and a single agent the user @mentioned in their message is used instead." },
                    "model_tier": { "type": "string", "enum": ["easy", "medium", "complex"] },
                    "start": { "type": "boolean" }
                }), vec!["title", "prompt"])
            },
            { "name": "start_task", "description": "Start a backlog task.",
              "inputSchema": obj(json!({ "task_id": { "type": "string" } }), vec!["task_id"]) },
            { "name": "list_tasks", "description": "List this project's tasks with status.",
              "inputSchema": obj(json!({}), vec![]) },
            { "name": "get_task_status", "description": "Status, cost, and latest run of one task.",
              "inputSchema": obj(json!({ "task_id": { "type": "string" } }), vec!["task_id"]) },
            { "name": "list_agents", "description": "List available agents in this workspace.",
              "inputSchema": obj(json!({}), vec![]) },
        ]
    })
}

async fn chat_project(state: &AppState, chat_id: Uuid) -> Result<(Uuid, Uuid), String> {
    let row = sqlx::query(
        "SELECT p.id AS project_id, p.workspace_id FROM chats c
         JOIN projects p ON p.id = c.project_id WHERE c.id = $1",
    )
    .bind(chat_id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok((row.get("project_id"), row.get("workspace_id")))
}

async fn call_tool(
    state: &AppState,
    chat_id: Uuid,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    let (project_id, workspace_id) = chat_project(state, chat_id).await?;
    match name {
        "create_task" => {
            let title = args
                .get("title")
                .and_then(Value::as_str)
                .ok_or("title is required")?;
            let prompt = args
                .get("prompt")
                .and_then(Value::as_str)
                .ok_or("prompt is required")?;
            let tier = match args.get("model_tier").and_then(Value::as_str) {
                Some(t @ ("easy" | "medium" | "complex")) => t,
                _ => "medium",
            };
            let (agent_id, agent_name) =
                resolve_agent(state, chat_id, workspace_id, args.get("agent_name")).await?;
            let start = args.get("start").and_then(Value::as_bool).unwrap_or(false);

            let row = sqlx::query(
                "INSERT INTO tasks (project_id, title, prompt, model_tier, agent_id, chat_id, board_column)
                 VALUES ($1,$2,$3,$4,$5,$6, CASE WHEN $7 THEN 'running' ELSE 'backlog' END)
                 RETURNING id",
            )
            .bind(project_id)
            .bind(title)
            .bind(prompt)
            .bind(tier)
            .bind(agent_id)
            .bind(chat_id)
            .bind(start)
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            let task_id: Uuid = row.get("id");

            let run_id = if start {
                Some(
                    state
                        .orchestrator
                        .enqueue_task(task_id)
                        .await
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            // The bound agent is echoed back so the assistant reports what
            // actually happened rather than what it asked for — those differ
            // exactly when it left `agent_name` off and the user's `@mention`
            // supplied it.
            Ok(json!({
                "task_id": task_id,
                "run_id": run_id,
                "started": start,
                "agent": agent_name,
            }))
        }
        "start_task" => {
            let task_id = parse_task_id(&args)?;
            ensure_task_in_project(state, task_id, project_id).await?;
            let run_id = state
                .orchestrator
                .enqueue_task(task_id)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("UPDATE tasks SET board_column='running' WHERE id=$1")
                .bind(task_id)
                .execute(&state.db.pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "run_id": run_id, "started": true }))
        }
        "list_tasks" => {
            let rows = sqlx::query(
                "SELECT t.id, t.title, t.board_column, r.status AS run_status
                 FROM tasks t
                 LEFT JOIN LATERAL (SELECT status FROM runs WHERE task_id=t.id
                                    ORDER BY created_at DESC LIMIT 1) r ON TRUE
                 WHERE t.project_id=$1 ORDER BY t.created_at DESC",
            )
            .bind(project_id)
            .fetch_all(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({
                "tasks": rows.iter().map(|r| json!({
                    "task_id": r.get::<Uuid, _>("id"),
                    "title": r.get::<String, _>("title"),
                    "column": r.get::<String, _>("board_column"),
                    "run_status": r.get::<Option<String>, _>("run_status"),
                })).collect::<Vec<_>>()
            }))
        }
        "get_task_status" => {
            let task_id = parse_task_id(&args)?;
            ensure_task_in_project(state, task_id, project_id).await?;
            let row = sqlx::query(
                "SELECT t.title, t.board_column, r.status, r.cost_usd, r.error_reason
                 FROM tasks t
                 LEFT JOIN LATERAL (SELECT * FROM runs WHERE task_id=t.id
                                    ORDER BY created_at DESC LIMIT 1) r ON TRUE
                 WHERE t.id=$1",
            )
            .bind(task_id)
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({
                "title": row.get::<String, _>("title"),
                "column": row.get::<String, _>("board_column"),
                "run_status": row.get::<Option<String>, _>("status"),
                "cost_usd": row.get::<Option<f64>, _>("cost_usd"),
                "error": row.get::<Option<String>, _>("error_reason"),
            }))
        }
        "list_agents" => {
            let rows = sqlx::query(
                "SELECT name, description, model_tier FROM agents WHERE workspace_id=$1
                 ORDER BY name ASC",
            )
            .bind(workspace_id)
            .fetch_all(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({
                "agents": rows.iter().map(|r| json!({
                    "name": r.get::<String, _>("name"),
                    "description": r.get::<String, _>("description"),
                    "model_tier": r.get::<String, _>("model_tier"),
                })).collect::<Vec<_>>()
            }))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Which agent a new task binds to: what the assistant asked for, or failing
/// that, who the user named with `@` in the message being answered.
///
/// Two behaviours worth stating, because both used to be absent:
///
/// * **An unknown `agent_name` is an error**, not a shrug. It used to resolve
///   to `NULL`, so a single typo produced an unassigned task while the
///   assistant cheerfully reported it had assigned one. The error lists the
///   real names, which is something the model can act on.
/// * **A single `@mention` binds even when the model forgets to pass it
///   through.** That is the whole point of resolving the mention at send time:
///   the user's instruction does not depend on the model relaying it. Two or
///   more mentions are left to the model, because "which of these two tasks is
///   whose" is a question only the request can answer — and the prompt block
///   `mentions::augment_prompt` adds tells it to answer it.
async fn resolve_agent(
    state: &AppState,
    chat_id: Uuid,
    workspace_id: Uuid,
    asked: Option<&Value>,
) -> Result<(Option<Uuid>, Option<String>), String> {
    if let Some(name) = asked.and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        // Matched without regard to case, because everything upstream is:
        // `@frontend` finds the agent called Frontend, and a model echoing the
        // user's own typing back must not turn a mention that already resolved
        // into a hard error. `agents_ws_name` is unique per workspace, and two
        // names differing only in case would be a library nobody could use.
        let row = sqlx::query(
            "SELECT id, name FROM agents WHERE workspace_id=$1 AND lower(name)=lower($2)",
        )
        .bind(workspace_id)
        .bind(name)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
        return match row {
            Some(r) => Ok((Some(r.get("id")), Some(r.get("name")))),
            None => {
                let known = sqlx::query("SELECT name FROM agents WHERE workspace_id=$1 ORDER BY name")
                    .bind(workspace_id)
                    .fetch_all(&state.db.pool)
                    .await
                    .map_err(|e| e.to_string())?
                    .iter()
                    .map(|r| format!("\"{}\"", r.get::<String, _>("name")))
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(if known.is_empty() {
                    format!("no agent named \"{name}\" — this workspace has no agents yet")
                } else {
                    format!("no agent named \"{name}\". The agents here are: {known}")
                })
            }
        };
    }

    let mentioned = aichip_core::runs::mentions::latest_for_chat(&state.db, chat_id)
        .await
        .map_err(|e| e.to_string())?;
    match mentioned.len() {
        1 => Ok((Some(mentioned[0].0), Some(mentioned[0].1.clone()))),
        _ => Ok((None, None)),
    }
}

fn parse_task_id(args: &Value) -> Result<Uuid, String> {
    args.get("task_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| "task_id must be a UUID".to_string())
}

async fn ensure_task_in_project(
    state: &AppState,
    task_id: Uuid,
    project_id: Uuid,
) -> Result<(), String> {
    let row = sqlx::query("SELECT project_id FROM tasks WHERE id=$1")
        .bind(task_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
    match row {
        Some(r) if r.get::<Uuid, _>("project_id") == project_id => Ok(()),
        Some(_) => Err("task belongs to a different project".into()),
        None => Err("no such task".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::tools_list;

    #[test]
    fn tools_list_exposes_the_five_workspace_tools() {
        let v = tools_list();
        let names: Vec<&str> = v["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["create_task", "start_task", "list_tasks", "get_task_status", "list_agents"]
        );
    }

    /// The schema is the only place the model learns these two rules, and both
    /// are behaviour `resolve_agent` will actually enforce — a description that
    /// drifts from it is how a model ends up retrying a call that can never
    /// succeed, or omitting an argument thinking something else will fill it in.
    #[test]
    fn create_task_tells_the_model_what_agent_name_does() {
        let v = tools_list();
        let described = v["tools"][0]["inputSchema"]["properties"]["agent_name"]["description"]
            .as_str()
            .unwrap();
        assert!(described.contains("unknown name is rejected"));
        assert!(described.contains("@mention"));
    }
}
