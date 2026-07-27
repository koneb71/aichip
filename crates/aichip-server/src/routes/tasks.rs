use super::{internal, ApiError};
use crate::AppState;
use aichip_shared::{ModelTier, PermissionMode};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tasks", get(list).post(create))
        .route("/tasks/{id}/start", post(start))
        .route("/tasks/{id}/diff", get(diff))
        .route("/tasks/{id}/merge", post(merge))
        .route("/runs/{id}/events", get(run_events))
        .route("/runs/{id}/pending-permissions", get(pending_permissions))
        .route("/runs/{id}/cancel", post(cancel_run))
        .route("/permissions/{request_id}/resolve", post(resolve_permission))
}

#[derive(Deserialize)]
struct TaskFilter {
    workspace_id: Option<Uuid>,
    project_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<TaskFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT t.id, t.title, t.prompt, t.model_tier, t.board_column, t.branch,
                t.project_id, t.agent_id, a.name AS agent_name, a.color AS agent_color,
                r.id AS run_id, r.status AS run_status, r.cost_usd, r.model
         FROM tasks t
         JOIN projects p ON p.id = t.project_id
         LEFT JOIN agents a ON a.id = t.agent_id
         LEFT JOIN LATERAL (
             SELECT * FROM runs WHERE task_id = t.id ORDER BY created_at DESC LIMIT 1
         ) r ON TRUE
         WHERE ($1::uuid IS NULL OR p.workspace_id = $1)
           AND ($2::uuid IS NULL OR t.project_id = $2)
         ORDER BY t.created_at DESC",
    )
    .bind(filter.workspace_id)
    .bind(filter.project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let tasks: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "title": r.get::<String, _>("title"),
                "modelTier": r.get::<String, _>("model_tier"),
                "boardColumn": r.get::<String, _>("board_column"),
                "branch": r.get::<Option<String>, _>("branch"),
                "projectId": r.get::<Uuid, _>("project_id"),
                "agentId": r.get::<Option<Uuid>, _>("agent_id"),
                "agentName": r.get::<Option<String>, _>("agent_name"),
                "agentColor": r.get::<Option<String>, _>("agent_color"),
                "runId": r.get::<Option<Uuid>, _>("run_id"),
                "runStatus": r.get::<Option<String>, _>("run_status"),
                "costUsd": r.get::<Option<f64>, _>("cost_usd"),
                "model": r.get::<Option<String>, _>("model"),
            })
        })
        .collect();
    Ok(Json(json!({ "tasks": tasks })))
}

#[derive(Deserialize)]
struct CreateTask {
    project_id: Uuid,
    title: String,
    prompt: String,
    #[serde(default)]
    model_tier: ModelTier,
    #[serde(default)]
    permission_mode: PermissionMode,
    #[serde(default)]
    start: bool,
    agent_id: Option<Uuid>,
    /// "claude-code" (default) or "mock" for demos/E2E.
    engine: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateTask>,
) -> Result<Json<Value>, ApiError> {
    let tier = serde_json::to_value(body.model_tier).unwrap();
    let mode = serde_json::to_value(body.permission_mode).unwrap();
    let row = sqlx::query(
        "INSERT INTO tasks (project_id, title, prompt, model_tier, permission_mode, engine, agent_id, board_column)
         VALUES ($1,$2,$3,$4,$5,$6,$7,'backlog') RETURNING id",
    )
    .bind(body.project_id)
    .bind(&body.title)
    .bind(&body.prompt)
    .bind(tier.as_str().unwrap())
    .bind(mode.as_str().unwrap())
    .bind(body.engine.as_deref().unwrap_or("claude-code"))
    .bind(body.agent_id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let task_id: Uuid = row.get("id");

    let run_id = if body.start {
        let id = state
            .orchestrator
            .enqueue_task(task_id)
            .await
            .map_err(internal)?;
        sqlx::query("UPDATE tasks SET board_column='running' WHERE id=$1")
            .bind(task_id)
            .execute(&state.db.pool)
            .await
            .map_err(internal)?;
        Some(id)
    } else {
        None
    };
    Ok(Json(json!({ "id": task_id, "runId": run_id })))
}

async fn start(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let run_id = state.orchestrator.enqueue_task(id).await.map_err(internal)?;
    sqlx::query("UPDATE tasks SET board_column='running' WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "runId": run_id })))
}

async fn diff(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT t.worktree_path, p.default_branch FROM tasks t
         JOIN projects p ON p.id = t.project_id WHERE t.id=$1",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let Some(worktree): Option<String> = row.get("worktree_path") else {
        return Ok(Json(json!({ "diff": "" })));
    };
    let base: String = row.get("default_branch");
    let diff = state
        .orchestrator
        .worktrees
        .diff(std::path::Path::new(&worktree), &base)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "diff": diff })))
}

async fn merge(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT t.title, t.worktree_path, t.branch, p.path AS project_path, p.default_branch
         FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id=$1",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let (Some(worktree), Some(branch)): (Option<String>, Option<String>) =
        (row.get("worktree_path"), row.get("branch"))
    else {
        return Err((StatusCode::BAD_REQUEST, "task has no worktree yet".into()));
    };
    let wt = aichip_core::worktrees::manager::Worktree {
        path: worktree.into(),
        branch,
    };
    let title: String = row.get("title");
    state
        .orchestrator
        .worktrees
        .squash_merge(
            std::path::Path::new(&row.get::<String, _>("project_path")),
            &wt,
            &row.get::<String, _>("default_branch"),
            &format!("aichip: {title}"),
        )
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    sqlx::query("UPDATE tasks SET board_column='done' WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "merged": true })))
}

async fn run_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT seq, type, payload, ts FROM events WHERE run_id=$1 ORDER BY seq ASC",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let events: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "seq": r.get::<i64, _>("seq"),
                "ts": r.get::<chrono::DateTime<chrono::Utc>, _>("ts"),
                "event": r.get::<Value, _>("payload"),
            })
        })
        .collect();
    Ok(Json(json!({ "events": events })))
}

/// Permission requests live in memory while the engine's MCP call blocks on
/// them, so a dashboard refresh needs to re-fetch anything still pending.
async fn pending_permissions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    let pending: Vec<Value> = state
        .permissions
        .pending_for_run(id)
        .into_iter()
        .map(|(request_id, tool_name, input)| {
            json!({ "requestId": request_id, "toolName": tool_name, "input": input })
        })
        .collect();
    Json(json!({ "pending": pending }))
}

async fn cancel_run(State(state): State<AppState>, Path(id): Path<Uuid>) -> Json<Value> {
    state.orchestrator.cancel(id);
    Json(json!({ "canceled": true }))
}

#[derive(Deserialize)]
struct Resolve {
    allowed: bool,
}

async fn resolve_permission(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
    Json(body): Json<Resolve>,
) -> Result<Json<Value>, ApiError> {
    if state.permissions.resolve(&request_id, body.allowed) {
        Ok(Json(json!({ "resolved": true })))
    } else {
        Err((StatusCode::NOT_FOUND, "no such pending permission".into()))
    }
}
