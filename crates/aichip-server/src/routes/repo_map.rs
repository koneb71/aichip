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
        .route("/projects/{id}/map/graph", get(graph))
        .route("/projects/{id}/map/file", get(file))
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

/// The nodes and edges, for the canvas to draw.
///
/// A strict read: unlike `status`, this never triggers a reconcile. Two
/// polling endpoints that each spawn one would put two passes over the same
/// project in flight on every tick, and they would fight over the same lock.
///
/// The whole graph in one response, deliberately: a layout cannot be computed
/// from a page of it, and this repository's is 194 nodes and 306 edges. If a
/// project ever outgrows that, the answer is a coarser graph — modules only —
/// and not a quietly truncated one.
async fn graph(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    repo_project(&state, project_id).await?;

    // Degrees come from the edge table rather than from counting client-side:
    // the number a node is sized by has to be the same number the inspector
    // prints, and deriving it twice is how those drift.
    let rows = sqlx::query(
        "SELECT d.id, d.rel_path, d.lang, d.bytes, d.rank, d.status,
                (SELECT count(*) FROM project_symbols s WHERE s.document_id = d.id) AS symbols,
                (SELECT count(*) FROM project_edges e WHERE e.to_document = d.id)   AS imported_by,
                (SELECT count(*) FROM project_edges e WHERE e.from_document = d.id) AS imports
         FROM project_documents d
         WHERE d.project_id = $1
         ORDER BY d.rel_path",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let edges = sqlx::query(
        "SELECT f.rel_path AS from_path, t.rel_path AS to_path, e.weight
         FROM project_edges e
         JOIN project_documents f ON f.id = e.from_document
         JOIN project_documents t ON t.id = e.to_document
         WHERE e.project_id = $1
         ORDER BY f.rel_path, t.rel_path",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    // Ordered by path on both queries, so the same repository produces the
    // same array twice — a layout computed from this must not move because a
    // database chose a different scan order.
    let index = sqlx::query(
        "SELECT head_sha, imports_total, imports_resolved, structure_version
         FROM project_index WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({
        "nodes": rows.iter().map(|r| json!({
            "path": r.get::<String, _>("rel_path"),
            "lang": r.get::<Option<String>, _>("lang"),
            "bytes": r.get::<i64, _>("bytes"),
            "rank": r.get::<f32, _>("rank"),
            "status": r.get::<String, _>("status"),
            "symbols": r.get::<i64, _>("symbols"),
            "importedBy": r.get::<i64, _>("imported_by"),
            "imports": r.get::<i64, _>("imports"),
        })).collect::<Vec<_>>(),
        "edges": edges.iter().map(|r| json!({
            "from": r.get::<String, _>("from_path"),
            "to": r.get::<String, _>("to_path"),
            "weight": r.get::<i32, _>("weight"),
        })).collect::<Vec<_>>(),
        // The honesty pair. An import that resolves to nothing draws no edge,
        // and a node with no edges reads as "nothing depends on this". Saying
        // how many were dropped is what lets a reader weigh an empty
        // neighbourhood instead of believing it.
        "importsTotal": index.as_ref().map(|r| r.get::<i32, _>("imports_total")).unwrap_or(0),
        "importsResolved": index.as_ref().map(|r| r.get::<i32, _>("imports_resolved")).unwrap_or(0),
        "structureVersion": index.as_ref().map(|r| r.get::<i64, _>("structure_version")).unwrap_or(0),
        "indexedSha": index.as_ref().and_then(|r| r.get::<Option<String>, _>("head_sha")),
    })))
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
}

/// One file's insides: what it defines, and what it asked for.
///
/// Fetched on selection rather than shipped with the graph — this repository's
/// 890 symbols are worth ~50KB that nobody looks at until they click.
async fn file(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    axum::extract::Query(q): axum::extract::Query<FileQuery>,
) -> Result<Json<Value>, ApiError> {
    repo_project(&state, project_id).await?;
    let doc: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM project_documents WHERE project_id = $1 AND rel_path = $2",
    )
    .bind(project_id)
    .bind(&q.path)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    let Some(doc) = doc else {
        return Err((
            StatusCode::NOT_FOUND,
            "that file is not in the index".into(),
        ));
    };

    let symbols = sqlx::query(
        "SELECT name, kind, line, signature FROM project_symbols
         WHERE document_id = $1 ORDER BY line",
    )
    .bind(doc)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    // Both directions. "What would I break" is the importers, and it is the
    // question a person actually opens a dependency graph to ask.
    let imports = sqlx::query(
        "SELECT t.rel_path AS path, e.weight FROM project_edges e
         JOIN project_documents t ON t.id = e.to_document
         WHERE e.from_document = $1 ORDER BY t.rel_path",
    )
    .bind(doc)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let importers = sqlx::query(
        "SELECT f.rel_path AS path, e.weight FROM project_edges e
         JOIN project_documents f ON f.id = e.from_document
         WHERE e.to_document = $1 ORDER BY f.rel_path",
    )
    .bind(doc)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    // Specifiers that resolved to nothing, named rather than hidden: an
    // external package looks exactly like a broken relative path once the edge
    // is dropped, and only one of those is a problem.
    let unresolved: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT specifier FROM project_imports
         WHERE document_id = $1 ORDER BY specifier",
    )
    .bind(doc)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({
        "path": q.path,
        "symbols": symbols.iter().map(|r| json!({
            "name": r.get::<String, _>("name"),
            "kind": r.get::<String, _>("kind"),
            "line": r.get::<i32, _>("line"),
            "signature": r.get::<Option<String>, _>("signature"),
        })).collect::<Vec<_>>(),
        "imports": imports.iter().map(|r| json!({
            "path": r.get::<String, _>("path"), "weight": r.get::<i32, _>("weight"),
        })).collect::<Vec<_>>(),
        "importers": importers.iter().map(|r| json!({
            "path": r.get::<String, _>("path"), "weight": r.get::<i32, _>("weight"),
        })).collect::<Vec<_>>(),
        "specifiers": unresolved,
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
    // Never gated on `embed::status()`. It reports what this process has asked
    // the embedder for, and the embedder only loads when something asks — so a
    // pre-check refuses every search after a restart, on an index that is
    // complete and sitting in the database. The first query pays the load.
    match aichip_core::rag::retrieve::top_k(
        &state.db,
        project_id,
        &body.q,
        body.limit.unwrap_or(12).min(30),
    )
    .await
    {
        Ok(hits) => Ok(Json(json!({
            "hits": hits.iter().map(|h| json!({
                "path": h.rel_path,
                "score": h.score,
                "line": h.start_line,
                "symbol": h.symbol,
                "excerpt": h.content.chars().take(400).collect::<String>(),
            })).collect::<Vec<_>>(),
        }))),
        // Named, not swallowed: a failed search that renders as "no results"
        // reads as "your code does not contain this", which is a lie.
        Err(e) => Ok(Json(json!({ "hits": [], "note": e.to_string() }))),
    }
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
