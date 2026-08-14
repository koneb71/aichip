use super::{attachments, internal, ApiError};
use crate::AppState;
use aichip_core::runs::mentions;
use aichip_shared::{ModelTier, ReasoningEffort};
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
        // General chats: scoped to a workspace, attached to no project. Same
        // rows, same turn machinery — the NULL project is what changes what
        // the assistant stands in and may reach for.
        .route("/workspaces/{id}/chats", get(list_general).post(open_general))
        .route("/workspaces/{id}/chats/new", post(new_general))
        .route("/chats/{id}", delete(delete_chat).patch(rename_chat))
        .route("/chats/{id}/messages", get(messages).post(send))
        .route("/chats/{id}/plan/{message_id}/approve", post(approve_plan))
        .route("/chats/{id}/questions/{question_id}/answer", post(answer_question))
}

async fn list_chats(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT c.id, c.title, c.updated_at, c.model_tier, c.effort, c.plan_mode,
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
                "modelTier": r.get::<Option<String>, _>("model_tier"),
                "effort": r.get::<Option<String>, _>("effort"),
                "planMode": r.get::<bool, _>("plan_mode"),
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

// ── General chats ───────────────────────────────────────────────────────────
//
// The same rows and the same turn machinery as a project chat, minus the
// project: the assistant stands in a scratch directory with the web instead
// of a checkout with the board tools. The three handlers mirror their
// project-scoped counterparts exactly — only the WHERE differs.

async fn list_general(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT c.id, c.title, c.updated_at, c.model_tier, c.effort, c.plan_mode,
                (SELECT count(*) FROM chat_messages m WHERE m.chat_id = c.id) AS message_count
         FROM chats c WHERE c.workspace_id=$1 AND c.project_id IS NULL
         ORDER BY c.updated_at DESC",
    )
    .bind(workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let chats: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "title": r.get::<String, _>("title"),
                "modelTier": r.get::<Option<String>, _>("model_tier"),
                "effort": r.get::<Option<String>, _>("effort"),
                "planMode": r.get::<bool, _>("plan_mode"),
                "messageCount": r.get::<i64, _>("message_count"),
                "updatedAt": r.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
            })
        })
        .collect();
    Ok(Json(json!({ "chats": chats })))
}

async fn open_general(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if let Some(row) = sqlx::query(
        "SELECT id FROM chats WHERE workspace_id=$1 AND project_id IS NULL
         ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(workspace_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    {
        return Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })));
    }
    let row = sqlx::query("INSERT INTO chats (workspace_id) VALUES ($1) RETURNING id")
        .bind(workspace_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })))
}

async fn new_general(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query("INSERT INTO chats (workspace_id) VALUES ($1) RETURNING id")
        .bind(workspace_id)
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
        // One aggregate rather than a query per message: the panel polls this
        // every 2.5s, so an N+1 here would be felt.
        "SELECT m.id, m.role, m.content, m.run_id, m.created_at,
                m.is_plan, m.plan_outcome, m.stopped,
                att.items AS attachments, pages.items AS articles
         FROM chat_messages m
         LEFT JOIN LATERAL (
             SELECT coalesce(json_agg(json_build_object(
                 'id', a.id, 'filename', a.filename, 'mime', a.mime,
                 'kind', a.kind, 'size', a.size_bytes
             ) ORDER BY a.created_at), '[]'::json) AS items
             FROM attachments a WHERE a.message_id = m.id
         ) att ON TRUE
         -- Which knowledge-base pages this turn was given. Sent back with the
         -- message, not just held on the way in: after the turn, the only way
         -- to know what the assistant was handed is what this says.
         LEFT JOIN LATERAL (
             SELECT coalesce(json_agg(json_build_object(
                 'id', k.id, 'title', k.title
             ) ORDER BY ma.position), '[]'::json) AS items
             FROM chat_message_articles ma
             JOIN kb_articles k ON k.id = ma.article_id
             WHERE ma.message_id = m.id
         ) pages ON TRUE
         WHERE m.chat_id=$1 ORDER BY m.created_at ASC",
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
                "attachments": r.get::<Value, _>("attachments"),
                "articles": r.get::<Value, _>("articles"),
                "isPlan": r.get::<bool, _>("is_plan"),
                "planOutcome": r.get::<Option<String>, _>("plan_outcome"),
                "stopped": r.get::<bool, _>("stopped"),
            })
        })
        .collect();

    let active = active_run(&state, chat_id).await?;

    // The open clarifying question, if there is one. Sent beside the messages
    // rather than attached to one: it belongs to the *conversation's* current
    // state, and reading it back this way is also what makes it survive a
    // refresh without any client-side memory.
    let question = sqlx::query(
        "SELECT id, questions FROM chat_questions
          WHERE chat_id = $1 AND answered_at IS NULL
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(chat_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .map(|r| json!({ "id": r.get::<Uuid, _>("id"), "questions": r.get::<Value, _>("questions") }));

    Ok(Json(json!({
        "messages": messages,
        "activeRunId": active,
        "openQuestion": question,
    })))
}

#[derive(Deserialize)]
struct AnswerQuestion {
    /// One list of chosen labels per question, in the order they were asked.
    /// Empty for a question left unanswered — the assistant is told to use its
    /// judgement rather than being handed a blank.
    #[serde(default)]
    answers: Vec<Vec<String>>,
}

/// Answer a clarifying question.
///
/// One action rather than "mark answered" plus "send a message": the two must
/// not come apart. An answer recorded without a turn leaves the assistant
/// waiting for something that already happened, and a turn sent without the
/// answer recorded leaves a live button offering to send it again.
async fn answer_question(
    State(state): State<AppState>,
    Path((chat_id, question_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AnswerQuestion>,
) -> Result<Json<Value>, ApiError> {
    if active_run(&state, chat_id).await?.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "the assistant is still working on the previous message".into(),
        ));
    }

    // Conditional, so a double click lands once — the second finds nothing
    // open. The `RETURNING` is what gives us the questions to phrase against.
    let row = sqlx::query(
        "UPDATE chat_questions SET answered_at = now(), answer = $3
          WHERE id = $1 AND chat_id = $2 AND answered_at IS NULL
        RETURNING questions",
    )
    .bind(question_id)
    .bind(chat_id)
    .bind(serde_json::to_value(&body.answers).map_err(internal)?)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((
        StatusCode::CONFLICT,
        "that question has already been answered".to_string(),
    ))?;

    let questions: Vec<aichip_core::runs::questions::Question> =
        serde_json::from_value(row.get("questions")).map_err(internal)?;
    let content = aichip_core::runs::questions::answer_message(&questions, &body.answers);

    let message = sqlx::query(
        "INSERT INTO chat_messages (chat_id, role, content) VALUES ($1, 'user', $2) RETURNING id",
    )
    .bind(chat_id)
    .bind(&content)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    sqlx::query("UPDATE chats SET updated_at = now() WHERE id = $1")
        .bind(chat_id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;

    let run_id = state
        .orchestrator
        .enqueue_chat_turn(chat_id, &state.orchestrator.default_engine())
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({ "messageId": message.get::<Uuid, _>("id"), "runId": run_id }),
    ))
}

#[derive(Deserialize)]
struct ApprovePlan {
    /// The plan as the person wants it carried out, when they changed it.
    /// Omitted means "as written" — the session already holds that text, and
    /// echoing it back would both spend the tokens twice and invite the
    /// assistant to read its own proposal as a fresh instruction.
    #[serde(default)]
    plan: Option<String>,
}

/// Carry out a plan.
///
/// Approving *leaves* plan mode, which is the whole point: the next turn is
/// the one that acts, so it needs the tools plan mode took away. Turning it
/// back on afterwards would be a second decision, and the toggle is right
/// there.
///
/// A plain turn rather than a resumed parked run, because a chat turn cannot
/// park: `active_run` counts every non-terminal run as active and refuses the
/// next message, so a parked plan would freeze the conversation it belongs to.
async fn approve_plan(
    State(state): State<AppState>,
    Path((chat_id, message_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ApprovePlan>,
) -> Result<Json<Value>, ApiError> {
    if active_run(&state, chat_id).await?.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "the assistant is still working on the previous message".into(),
        ));
    }

    // Only an open plan, and only this chat's. Doing it as one conditional
    // update rather than a read-then-write is what makes a double click
    // land once: the second finds no open plan and is refused.
    let claimed = sqlx::query(
        "UPDATE chat_messages SET plan_outcome = 'approved'
          WHERE id = $1 AND chat_id = $2 AND is_plan AND plan_outcome IS NULL",
    )
    .bind(message_id)
    .bind(chat_id)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    if claimed.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            "that plan has already been answered".into(),
        ));
    }

    sqlx::query("UPDATE chats SET plan_mode = false, updated_at = now() WHERE id = $1")
        .bind(chat_id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;

    let row = sqlx::query(
        "INSERT INTO chat_messages (chat_id, role, content) VALUES ($1, 'user', $2) RETURNING id",
    )
    .bind(chat_id)
    .bind(aichip_core::runs::chat_plan::approval(
        body.plan.as_deref().map(str::trim).filter(|p| !p.is_empty()),
    ))
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    let run_id = state
        .orchestrator
        .enqueue_chat_turn(chat_id, &state.orchestrator.default_engine())
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({ "messageId": row.get::<Uuid, _>("id"), "runId": run_id }),
    ))
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
    /// Engine id from `/api/engines`. Omitted means the machine default.
    engine: Option<String>,
    /// Which model, and how hard it thinks. Both stick to the chat rather than
    /// the message: picking "think harder" and having it last exactly one turn
    /// would be a strange thing to have chosen.
    /// Typed, so a client that invents a level is a 422 rather than a row the
    /// orchestrator has to shrug at when the turn actually runs.
    model_tier: Option<ModelTier>,
    effort: Option<ReasoningEffort>,
    /// Propose rather than act. Sticks to the chat like the two above — plan
    /// mode is a mode you are in, not a property of one sentence.
    plan_mode: Option<bool>,
    /// Ids from POST /api/projects/{id}/attachments, bound to this message.
    #[serde(default)]
    attachment_ids: Vec<Uuid>,
    /// Knowledge-base pages to put in front of the assistant for this turn.
    /// Workspace-scoped rather than project-scoped, so a general chat can
    /// carry them even though it cannot carry a file.
    #[serde(default)]
    article_ids: Vec<Uuid>,
}

async fn send(
    State(state): State<AppState>,
    Path(chat_id): Path<Uuid>,
    Json(body): Json<SendBody>,
) -> Result<Json<Value>, ApiError> {
    // "Look at this screenshot" with no words is a legitimate turn, so only
    // reject a message that is empty *and* carries nothing.
    if body.content.trim().is_empty()
        && body.attachment_ids.is_empty()
        && body.article_ids.is_empty()
    {
        return Err((StatusCode::BAD_REQUEST, "message is empty".into()));
    }
    // Remembered on the chat, not the turn — see SendBody. `coalesce` so a
    // client that only sends `content` does not silently reset the choice.
    if body.model_tier.is_some() || body.effort.is_some() || body.plan_mode.is_some() {
        sqlx::query(
            "UPDATE chats SET model_tier = coalesce($2, model_tier),
                              effort = coalesce($3, effort),
                              plan_mode = coalesce($4, plan_mode)
             WHERE id = $1",
        )
        .bind(chat_id)
        .bind(body.model_tier.and_then(|t| serde_json::to_value(t).ok()).and_then(|v| v.as_str().map(str::to_string)))
        .bind(body.effort.map(|e| e.as_str().to_string()))
        .bind(body.plan_mode)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    }

    // Serialize turns: --resume forks sessions, so two concurrent turns
    // would race on chats.session_id.
    if active_run(&state, chat_id).await?.is_some() {
        return Err((
            StatusCode::CONFLICT,
            "the assistant is still working on the previous message".into(),
        ));
    }

    // The claim needs the owning project, and resolving `@agent` needs the
    // workspace above it — only the chat row knows either. A general chat has
    // no project; its workspace is on the row itself.
    let owner = sqlx::query(
        "SELECT c.project_id, COALESCE(p.workspace_id, c.workspace_id) AS workspace_id
         FROM chats c LEFT JOIN projects p ON p.id = c.project_id WHERE c.id=$1",
    )
    .bind(chat_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such chat".to_string()))?;
    let project_id: Option<Uuid> = owner.get("project_id");
    let workspace_id: Uuid = owner.get("workspace_id");
    // Attachments are project machinery: the files live under the project and
    // the claim binds them there. Refused rather than dropped, so the person
    // who dragged a screenshot in learns why it did not arrive.
    if project_id.is_none() && !body.attachment_ids.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "attachments need a project — this is a general chat".into(),
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

    // Who the user named with `@`, decided from the agent library rather than
    // from anything the client sent. The dashboard parses the same text to draw
    // the chips, but what binds a task is this row.
    let (mentioned, skills) = mentions::resolve_all(&state.db, workspace_id, &body.content)
        .await
        .map_err(internal)?;
    mentions::record(&state.db, message_id, &mentioned)
        .await
        .map_err(internal)?;
    mentions::record_skills(&state.db, message_id, &skills)
        .await
        .map_err(internal)?;
    // Scoped to the workspace at write time as well as at read time — a page
    // from somebody else's workspace is not written down at all, so a later
    // reader of this table sees what was actually attached.
    aichip_core::kb::record_for_message(&state.db, message_id, workspace_id, &body.article_ids)
        .await
        .map_err(internal)?;

    if let Some(project_id) = project_id {
        attachments::claim(
            &state.db,
            &body.attachment_ids,
            project_id,
            attachments::Owner::Message(message_id),
        )
        .await?;
    }

    // Name the chat after its opening line, and float it to the top of the
    // list. Only untitled chats are renamed, so a user's own title sticks.
    sqlx::query(
        "UPDATE chats SET updated_at=now(),
                title = CASE WHEN title=$2 THEN $3 ELSE title END
         WHERE id=$1",
    )
    .bind(chat_id)
    .bind(UNTITLED)
    .bind(chat_title_for(&body.content, message_id, &state).await?)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;

    let run_id = state
        .orchestrator
        .enqueue_chat_turn(
            chat_id,
            body.engine.as_deref().unwrap_or(&state.orchestrator.default_engine()),
        )
        .await
        .map_err(internal)?;

    Ok(Json(json!({ "messageId": message_id, "runId": run_id })))
}

/// Title for an untitled chat. Normally the message's opening line, but an
/// attachment-only turn has no text — fall back to what was attached, so those
/// chats aren't all called "Chat".
async fn chat_title_for(
    content: &str,
    message_id: Uuid,
    state: &AppState,
) -> Result<String, ApiError> {
    let title = derive_title(content);
    if title != UNTITLED {
        return Ok(title);
    }
    let first: Option<(String,)> = sqlx::query_as(
        "SELECT filename FROM attachments WHERE message_id=$1 ORDER BY created_at LIMIT 1",
    )
    .bind(message_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(first.map(|(f,)| derive_title(&f)).unwrap_or_else(|| UNTITLED.to_string()))
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
