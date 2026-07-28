use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

/// Title given to a chat until its first user message names it.
const UNTITLED: &str = "Chat";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{id}/chats", get(list_chats).post(open_chat))
        .route("/projects/{id}/chats/new", post(new_chat))
        .route("/chats/{id}", delete(delete_chat).patch(rename_chat))
        .route("/chats/{id}/messages", get(messages).post(send))
}

async fn list_chats(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT c.id, c.title, c.updated_at,
                (SELECT count(*) FROM chat_messages m WHERE m.chat_id = c.id) AS message_count
         FROM chats c WHERE c.project_id=$1 ORDER BY c.updated_at DESC",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let chats: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "title": r.get::<String, _>("title"),
                "messageCount": r.get::<i64, _>("message_count"),
                "updatedAt": r.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
            })
        })
        .collect();
    Ok(Json(json!({ "chats": chats })))
}

/// Always start a fresh conversation. Distinct from `open_chat`, which
/// reuses the most recent one — a new chat means a new CLI session, so the
/// assistant starts without the previous thread's context.
async fn new_chat(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query("INSERT INTO chats (project_id) VALUES ($1) RETURNING id")
        .bind(project_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })))
}

#[derive(Deserialize)]
struct RenameBody {
    title: String,
}

async fn rename_chat(
    State(state): State<AppState>,
    Path(chat_id): Path<Uuid>,
    Json(body): Json<RenameBody>,
) -> Result<Json<Value>, ApiError> {
    let title = body.title.trim();
    if title.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "title is empty".into()));
    }
    sqlx::query("UPDATE chats SET title=$2 WHERE id=$1")
        .bind(chat_id)
        .bind(title)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "id": chat_id, "title": title })))
}

async fn delete_chat(
    State(state): State<AppState>,
    Path(chat_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    // A chat mid-turn owns a running CLI process; deleting the row would
    // orphan it and the run would write back to a chat that no longer exists.
    if active_run(&state, chat_id).await?.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "the assistant is still working in this chat".into(),
        ));
    }
    sqlx::query("DELETE FROM chats WHERE id=$1")
        .bind(chat_id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
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

    // Name the chat after its opening line, and float it to the top of the
    // list. Only untitled chats are renamed, so a user's own title sticks.
    sqlx::query(
        "UPDATE chats SET updated_at=now(),
                title = CASE WHEN title=$2 THEN $3 ELSE title END
         WHERE id=$1",
    )
    .bind(chat_id)
    .bind(UNTITLED)
    .bind(derive_title(&body.content))
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;

    let run_id = state
        .orchestrator
        .enqueue_chat_turn(chat_id, body.engine.as_deref().unwrap_or("claude-code"))
        .await
        .map_err(internal)?;

    Ok(Json(json!({ "messageId": message_id, "runId": run_id })))
}

/// A chat title from its first message: the opening line, clipped to
/// something that fits the sidebar.
fn derive_title(content: &str) -> String {
    const MAX_CHARS: usize = 48;
    let line = content.trim().lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return UNTITLED.to_string();
    }
    // Count characters, not bytes — clipping mid-codepoint would panic.
    if line.chars().count() <= MAX_CHARS {
        return line.to_string();
    }
    let clipped: String = line.chars().take(MAX_CHARS).collect();
    // Prefer a word boundary when there is one reasonably close to the end.
    match clipped.rsplit_once(' ') {
        Some((head, _)) if head.chars().count() >= MAX_CHARS / 2 => format!("{head}…"),
        _ => format!("{clipped}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::{derive_title, UNTITLED};

    #[test]
    fn titles_come_from_the_first_line() {
        assert_eq!(derive_title("fix the flaky login test"), "fix the flaky login test");
        assert_eq!(derive_title("  padded  \nsecond line"), "padded");
        assert_eq!(derive_title("   "), UNTITLED);
    }

    #[test]
    fn long_titles_clip_on_a_word_boundary() {
        let long = "please refactor the authentication middleware so it stops leaking sessions";
        let title = derive_title(long);
        assert!(title.ends_with('…'));
        assert!(title.chars().count() <= 49);
        assert!(long.starts_with(title.trim_end_matches('…')));
    }

    #[test]
    fn clipping_does_not_split_multibyte_characters() {
        // Would panic on a byte-index slice.
        let title = derive_title(&"日".repeat(80));
        assert!(title.chars().count() <= 49);
    }
}
