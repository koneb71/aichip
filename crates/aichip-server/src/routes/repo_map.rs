//! The project's code index: its state, and search over it.
//!
//! Reading the index is what keeps it honest. `status` is the poll target and
//! is cheap by construction — one row and a count — while `search` is the
//! feature: finding code by what it does when you do not know what it is
//! called. Grep answers "where is `vet_task`"; this answers "where do we
//! decide whether a card can start", which is the same question asked by
//! somebody who has not read the file yet.
//!
//! Every reindex trigger is a background spawn except the button, which is
//! awaited — the button exists to answer "did it work".

use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{id}/map/status", get(status))
        .route("/projects/{id}/map/search", post(search))
        .route("/projects/{id}/map/reindex", post(reindex))
}

/// Refuse a project kind this cannot mean anything for.
///
/// A space already has its own document index on the same tables; running the
/// code enumerator over it would fight that. Returning the project's path is
/// how every caller here proves the project exists at the same time.
async fn repo_project(state: &AppState, project_id: Uuid) -> Result<String, ApiError> {
    let row = sqlx::query("SELECT path, kind FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    if row.get::<String, _>("kind") == "space" {
        return Err((
            StatusCode::BAD_REQUEST,
            "a document space has its own index — this is for code".into(),
        ));
    }
    Ok(row.get("path"))
}

/// Where the index stands, and whether it still describes the tree.
///
/// Also the on-open trigger: reading this is what keeps the index honest about
/// changes made outside aichip, exactly as loading the documents panel does
/// for a space. Spawned, so opening the page never waits on a reconcile.
async fn status(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    repo_project(&state, project_id).await?;

    let row = sqlx::query(
        "SELECT phase, head_sha, structure_version, files_total, files_parsed,
                files_embedded, error, note, indexed_at
         FROM project_index WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;

    let phase = row
        .as_ref()
        .map(|r| r.get::<String, _>("phase"))
        .unwrap_or_else(|| "never".to_string());

    // Kick a background pass when the tree has moved or nothing has ever been
    // read. `is_stale` is one `git rev-parse`, so asking on every poll is
    // cheaper than the reconcile it avoids.
    let running = matches!(phase.as_str(), "structure" | "embedding");
    if !running && aichip_core::repo::index::is_stale(&state.db, project_id).await {
        let db = state.db.clone();
        tokio::spawn(async move {
            if let Err(e) = aichip_core::repo::index::reconcile(&db, project_id).await {
                tracing::warn!(%project_id, error = %e, "code index reconcile failed");
            }
        });
    }

    let indexed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project_documents WHERE project_id = $1 AND status = 'indexed'",
    )
    .bind(project_id)
    .fetch_one(&state.db.pool)
    .await
    .unwrap_or(0);

    Ok(Json(json!({
        "phase": phase,
        "embedder": aichip_core::rag::embed::status(),
        "counts": {
            "files": row.as_ref().map(|r| r.get::<i32, _>("files_total")).unwrap_or(0),
            "parsed": row.as_ref().map(|r| r.get::<i32, _>("files_parsed")).unwrap_or(0),
            "embedded": indexed,
        },
        "structureVersion": row.as_ref().map(|r| r.get::<i64, _>("structure_version")).unwrap_or(0),
        "indexedSha": row.as_ref().and_then(|r| r.get::<Option<String>, _>("head_sha")),
        "indexedAt": row
            .as_ref()
            .and_then(|r| r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("indexed_at"))
            .map(|t| t.to_rfc3339()),
        "error": row.as_ref().and_then(|r| r.get::<Option<String>, _>("error")),
        "note": row.as_ref().and_then(|r| r.get::<Option<String>, _>("note")),
    })))
}

#[derive(Deserialize)]
struct SearchBody {
    q: String,
    #[serde(default)]
    limit: Option<usize>,
}

/// Find code by meaning.
///
/// POST rather than GET because a question is a body: a natural-language query
/// in a URL ends up in access logs and in shell history.
async fn search(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<SearchBody>,
) -> Result<Json<Value>, ApiError> {
    repo_project(&state, project_id).await?;
    let ready = matches!(
        aichip_core::rag::embed::status(),
        aichip_core::rag::embed::EmbedStatus::Ready
    );
    // Not an error: an index that has not embedded yet has nothing to say by
    // meaning, and the caller has a path search that still works.
    if !ready {
        return Ok(Json(json!({ "hits": [], "embedderReady": false })));
    }
    let hits = aichip_core::rag::retrieve::top_k(
        &state.db,
        project_id,
        &body.q,
        body.limit.unwrap_or(12).min(30),
    )
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "hits": hits.iter().map(|h| json!({
            "path": h.rel_path,
            "score": h.score,
            "line": h.start_line,
            "symbol": h.symbol,
            "excerpt": h.content.chars().take(400).collect::<String>(),
        })).collect::<Vec<_>>(),
        "embedderReady": true,
    })))
}

/// Read it all again, now, and say what happened.
async fn reindex(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    repo_project(&state, project_id).await?;
    let report = aichip_core::repo::index::reconcile(&state.db, project_id)
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}
