use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{id}/chats", get(list_chats).post(open_chat))
        .route("/chats/{id}/messages", get(messages).post(send))
}

async fn list_chats(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, title, created_at FROM chats WHERE project_id=$1 ORDER BY updated_at DESC",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let chats: Vec<Value> = rows
        .iter()
        .map(|r| json!({ "id": r.get::<Uuid, _>("id"), "title": r.get::<String, _>("title") }))
        .collect();
    Ok(Json(json!({ "chats": chats })))
}

/// Open a chat for a project: returns the most recent one, creating it if
/// the project has none (one primary chat per project for now).
async fn open_chat(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if let Some(row) = sqlx::query(
        "SELECT id FROM chats WHERE project_id=$1 ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    {
        return Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })));
    }
    let row = sqlx::query("INSERT INTO chats (project_id) VALUES ($1) RETURNING id")
        .bind(project_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })))
}

async fn messages(
    State(state): State<AppState>,
    Path(chat_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, role, content, run_id, created_at FROM chat_messages
         WHERE chat_id=$1 ORDER BY created_at ASC",
    )
    .bind(chat_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let messages: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "role": r.get::<String, _>("role"),
                "content": r.get::<String, _>("content"),
                "runId": r.get::<Option<Uuid>, _>("run_id"),
                "ts": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        })
        .collect();

    let active = active_run(&state, chat_id).await?;
    Ok(Json(json!({ "messages": messages, "activeRunId": active })))
}

async fn active_run(state: &AppState, chat_id: Uuid) -> Result<Option<Uuid>, ApiError> {
    let row = sqlx::query(
        "SELECT id FROM runs WHERE chat_id=$1
         AND status NOT IN ('completed','failed','canceled')
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(chat_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(row.map(|r| r.get("id")))
}

#[derive(Deserialize)]
struct SendBody {
    content: String,
    /// "claude-code" (default) or "mock" for demos/E2E.
    engine: Option<String>,
}

async fn send(
    State(state): State<AppState>,
    Path(chat_id): Path<Uuid>,
    Json(body): Json<SendBody>,
) -> Result<Json<Value>, ApiError> {
    if body.content.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message is empty".into()));
    }
    // Serialize turns: --resume forks sessions, so two concurrent turns
    // would race on chats.session_id.
    if active_run(&state, chat_id).await?.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "the assistant is still working on the previous message".into(),
        ));
    }

    let row = sqlx::query(
        "INSERT INTO chat_messages (chat_id, role, content) VALUES ($1, 'user', $2) RETURNING id",
    )
    .bind(chat_id)
    .bind(body.content.trim())
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let message_id: Uuid = row.get("id");

    let run_id = state
        .orchestrator
        .enqueue_chat_turn(chat_id, body.engine.as_deref().unwrap_or("claude-code"))
        .await
        .map_err(internal)?;

    Ok(Json(json!({ "messageId": message_id, "runId": run_id })))
}
