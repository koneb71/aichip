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
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/docker", get(docker_status))
        .route("/previews/limits", get(get_limits).put(set_limits))
        .route("/previews/disk", get(disk).delete(reclaim))
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
