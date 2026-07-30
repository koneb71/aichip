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
