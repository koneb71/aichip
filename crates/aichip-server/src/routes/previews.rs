//! Start, read and stop a card's preview.
//!
//! Three verbs on one resource, because a card has at most one preview and
//! "the preview of task X" is the only way anything here is ever addressed.
//!
//! `POST` returns as soon as the row exists rather than when the container is
//! up: a `docker build` of a real project takes minutes, and a button that
//! hangs for them is a button people press twice.

use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/docker", get(docker_status))
        .route("/previews/limits", get(get_limits).put(set_limits))
        .route("/previews/disk", get(disk).delete(reclaim))
        .route(
            "/projects/{id}/preview",
            get(current_base).post(start_base).delete(stop_base),
        )
        .route(
            "/projects/{id}/preview-recipe",
            get(get_recipe).post(propose_recipe).put(approve_recipe),
        )
        .route(
            "/tasks/{id}/preview",
            get(current).post(start).delete(stop),
        )
}

/// Whether previews are possible at all on this machine.
///
/// Probed live, not cached at boot, for the same reason `/api/github` is:
/// Docker Desktop gets started after aichip just as often as before it, and
/// the point of showing this is to say "go and start it".
async fn docker_status() -> Json<Value> {
    match aichip_core::previews::docker::detect().await {
        None => Json(json!({
            "installed": false,
            "usable": false,
            "problem": "Docker isn't installed, or isn't on this machine's PATH.",
        })),
        // Installed but not answering — a completely different fix from not
        // having it, so it gets a different message.
        Some(Err(problem)) => Json(json!({
            "installed": true,
            "usable": false,
            "problem": format!("Docker is installed but its daemon isn't responding. {problem}"),
        })),
        Some(Ok(version)) => Json(json!({
            "installed": true,
            "usable": true,
            "version": version,
        })),
    }
}

/// The two numbers that decide whether previews are safe to forget about,
/// alongside what they are currently costing.
async fn get_limits(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let limits = aichip_core::previews::limits(&state.db).await;
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM previews WHERE status IN ('building','running')",
    )
    .fetch_one(&state.db.pool)
    .await
    .unwrap_or(0);
    Ok(Json(json!({
        "maxLive": limits.max_live,
        "idleMinutes": limits.idle_minutes,
        "live": live,
    })))
}

#[derive(serde::Deserialize)]
struct LimitsBody {
    max_live: i64,
    /// Zero means never idle-stop.
    idle_minutes: i64,
}

async fn set_limits(
    State(state): State<AppState>,
    Json(body): Json<LimitsBody>,
) -> Result<Json<Value>, ApiError> {
    // Clamped in the core rather than rejected here: a slider that refuses is
    // worse than one that stops at its own end.
    let saved = aichip_core::previews::set_limits(
        &state.db,
        aichip_core::previews::Limits {
            max_live: body.max_live,
            idle_minutes: body.idle_minutes,
        },
    )
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "maxLive": saved.max_live,
        "idleMinutes": saved.idle_minutes,
    })))
}

async fn disk(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let (bytes, reclaimable) = aichip_core::previews::disk(&state.db)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "bytes": bytes, "reclaimable": reclaimable })))
}

async fn reclaim(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let freed = aichip_core::previews::reclaim_disk(&state.db)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "reclaimed": freed })))
}

/// The project's recipe and whether anyone has approved it.
async fn get_recipe(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT dockerfile, status, edited FROM preview_recipes WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "recipe": row.map(|r| json!({
            "dockerfile": r.get::<String, _>("dockerfile"),
            "status": r.get::<String, _>("status"),
            "edited": r.get::<bool, _>("edited"),
        })),
    })))
}

/// Ask an agent to write one. Stored as a proposal — never built.
async fn propose_recipe(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let path: String = sqlx::query_scalar("SELECT path FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;

    // Gathered here rather than by the agent: it gets no tools and never runs
    // in the project, so what it saw is exactly what the reviewer can see, and
    // a file in the repository cannot talk it into anything on the way past.
    let survey = aichip_core::previews::recipe_writer::survey(std::path::Path::new(&path))
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("could not read the project: {e}")))?;

    let engine_id = state.orchestrator.default_engine();
    let engine = state
        .orchestrator
        .engine(&engine_id)
        .ok_or((StatusCode::PRECONDITION_FAILED, "no engine available".to_string()))?;
    let model_id = state
        .orchestrator
        .model_for(&engine_id, aichip_shared::ModelTier::Complex);

    let reply = aichip_core::runs::utility::utility_run(
        engine,
        model_id,
        aichip_core::previews::recipe_writer::prompt(&survey),
        Some(aichip_shared::ReasoningEffort::High),
        std::time::Duration::from_secs(240),
    )
    .await
    .map_err(internal)?;

    let Some(dockerfile) = aichip_core::previews::recipe_writer::extract(&reply) else {
        return Err((
            StatusCode::BAD_GATEWAY,
            "The agent did not return a Dockerfile. Try again, or write one yourself              and put it in the project."
                .into(),
        ));
    };

    // Replaces any previous proposal, and resets approval: text nobody has read
    // must never inherit the approval given to different text.
    sqlx::query(
        "INSERT INTO preview_recipes (project_id, dockerfile, status, edited, approved_at)
         VALUES ($1, $2, 'proposed', FALSE, NULL)
         ON CONFLICT (project_id) DO UPDATE
            SET dockerfile = EXCLUDED.dockerfile, status = 'proposed',
                edited = FALSE, approved_at = NULL, created_at = now()",
    )
    .bind(project_id)
    .bind(&dockerfile)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({
        "recipe": { "dockerfile": dockerfile, "status": "proposed", "edited": false },
    })))
}

#[derive(serde::Deserialize)]
struct ApproveBody {
    /// The text being approved. Sent back in full rather than approving "the
    /// current proposal" by reference, so an edit and an approval are one act
    /// and there is no window where a different text gets the nod.
    dockerfile: String,
}

async fn approve_recipe(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<ApproveBody>,
) -> Result<Json<Value>, ApiError> {
    let text = body.dockerfile.trim().to_string();
    if !text
        .lines()
        .any(|l| l.trim_start().to_ascii_uppercase().starts_with("FROM "))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "That is not a Dockerfile — it has no FROM line.".into(),
        ));
    }
    let edited: bool = sqlx::query_scalar(
        "SELECT dockerfile IS DISTINCT FROM $2 FROM preview_recipes WHERE project_id = $1",
    )
    .bind(project_id)
    .bind(&text)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .unwrap_or(true);

    sqlx::query(
        "INSERT INTO preview_recipes (project_id, dockerfile, status, edited, approved_at)
         VALUES ($1, $2, 'approved', $3, now())
         ON CONFLICT (project_id) DO UPDATE
            SET dockerfile = EXCLUDED.dockerfile, status = 'approved',
                edited = EXCLUDED.edited, approved_at = now()",
    )
    .bind(project_id)
    .bind(&text)
    .bind(edited)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({ "approved": true, "edited": edited })))
}

/// The project's base-branch preview — what a card's changes are compared to.
async fn current_base(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let preview = aichip_core::previews::get_base(&state.db, project_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "preview": preview })))
}

async fn start_base(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !docker_ready(&state).await {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            "Docker isn't available on this machine.".into(),
        ));
    }
    let preview = aichip_core::previews::start_base(&state.db, project_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "preview": preview })))
}

async fn stop_base(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let stopped = aichip_core::previews::stop_base(&state.db, project_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "stopped": stopped })))
}

async fn docker_ready(_state: &AppState) -> bool {
    matches!(
        aichip_core::previews::docker::detect().await,
        Some(Ok(_))
    )
}

async fn current(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let preview = aichip_core::previews::get(&state.db, task_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "preview": preview })))
}

async fn start(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    // Checked here rather than left to fail inside the build, so "you have no
    // Docker" is an immediate answer instead of a failed row.
    match aichip_core::previews::docker::detect().await {
        None => {
            return Err((
                StatusCode::PRECONDITION_FAILED,
                "Docker isn't installed on this machine, so there is nothing to build with."
                    .into(),
            ))
        }
        Some(Err(problem)) => {
            return Err((
                StatusCode::PRECONDITION_FAILED,
                format!("Docker isn't responding. {problem}"),
            ))
        }
        Some(Ok(_)) => {}
    }

    let preview = aichip_core::previews::start(&state.db, task_id)
        .await
        // These are all things the user can act on — no Dockerfile, no
        // worktree — so they are the message, not a 500 with a log line.
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "preview": preview })))
}

async fn stop(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let stopped = aichip_core::previews::stop(&state.db, task_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "stopped": stopped })))
}
