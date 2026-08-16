//! A space's documents: upload, list, delete, and the semantic index over
//! them.
//!
//! Every handler refuses a non-space project with a **409** — the project
//! exists, the operation conflicts with what it is. Uploads write into the
//! project's folder in place, and doing that to a repository checkout is
//! exactly the foot-gun the space/repo split exists to prevent.

use std::path::PathBuf;

use super::{attachments::sanitize_filename, internal, ApiError};
use crate::AppState;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

/// Whole-request and per-file caps, matching attachments until real corpora
/// demand more.
const MAX_UPLOAD_BYTES: usize = 26 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 10 * 1024 * 1024;

/// What a space accepts. All of it indexes now: text formats verbatim, PDF /
/// Word / PowerPoint / Excel through `rag::extract`. Legacy .doc/.ppt are
/// refused at upload — there is no reader for them, and refusing with the
/// fix beats accepting a file that can only ever sit `unsupported`.
const ALLOWED_EXT: &[&str] = &[
    "md", "txt", "csv", "json", "log", "pdf", "docx", "pptx", "xlsx", "xlsm", "xls", "ods",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/documents",
            get(list)
                .post(upload)
                .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        )
        .route("/projects/{id}/documents/reindex", post(reindex))
        .route("/projects/{id}/documents/status", get(status))
        .route("/projects/{id}/documents/{doc_id}", delete(remove))
}

/// The space's folder, or the 409 that explains why not.
async fn space_path(state: &AppState, project_id: Uuid) -> Result<PathBuf, ApiError> {
    let row = sqlx::query("SELECT path, kind FROM projects WHERE id=$1")
        .bind(project_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    if row.get::<String, _>("kind") != "space" {
        return Err((
            StatusCode::CONFLICT,
            "this project is a repository — documents live in spaces".into(),
        ));
    }
    Ok(PathBuf::from(row.get::<String, _>("path")))
}

fn doc_json(r: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": r.get::<Uuid, _>("id"),
        "relPath": r.get::<String, _>("rel_path"),
        "status": r.get::<String, _>("status"),
        "error": r.get::<Option<String>, _>("error"),
        "bytes": r.get::<i64, _>("bytes"),
        "indexedAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("indexed_at"),
    })
}

async fn list_rows(state: &AppState, project_id: Uuid) -> Result<Vec<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, rel_path, status, error, bytes, indexed_at
         FROM project_documents WHERE project_id=$1 ORDER BY rel_path",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(rows.iter().map(doc_json).collect())
}

/// List the documents — and reconcile in the background, so opening the
/// panel is what keeps the index honest about files added outside the app.
async fn list(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let dir = space_path(&state, project_id).await?;
    let docs = list_rows(&state, project_id).await?;
    let db = state.db.clone();
    tokio::spawn(async move {
        if let Err(e) = aichip_core::rag::index::reconcile(&db, project_id, &dir).await {
            tracing::warn!(%project_id, error=%e, "space reconcile failed");
        }
    });
    Ok(Json(json!({ "documents": docs })))
}

async fn upload(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let dir = space_path(&state, project_id).await?;

    let mut stored = 0usize;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let Some(name) = field.file_name().and_then(sanitize_filename) else {
            return Err((StatusCode::BAD_REQUEST, "a file needs a usable name".into()));
        };
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !ALLOWED_EXT.contains(&ext.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{name}: only {} files are accepted", ALLOWED_EXT.join(", ")),
            ));
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err((StatusCode::BAD_REQUEST, format!("{name} is over 10 MB")));
        }
        // The same honesty check attachments make: an extension is a claim,
        // and a renamed binary must not smuggle itself in as .md. The office
        // formats are ZIP containers; classic .xls is an OLE compound file.
        let ok = match ext.as_str() {
            "pdf" => bytes.starts_with(b"%PDF-"),
            "docx" | "pptx" | "xlsx" | "xlsm" | "ods" => bytes.starts_with(b"PK\x03\x04"),
            "xls" => bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]),
            _ => !bytes.contains(&0),
        };
        if !ok {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{name} does not look like a .{ext} file"),
            ));
        }

        // Collision → suffix, the create_space rule: two uploads named
        // notes.md are two documents, not one silently overwritten.
        let mut target = dir.join(&name);
        let (stem, dot_ext) = match name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (name.clone(), String::new()),
        };
        let mut n = 2;
        while target.exists() {
            target = dir.join(format!("{stem}-{n}{dot_ext}"));
            n += 1;
        }
        tokio::fs::write(&target, &bytes).await.map_err(internal)?;
        stored += 1;
    }
    if stored == 0 {
        return Err((StatusCode::BAD_REQUEST, "no files in the request".into()));
    }

    // Index in the background; the rows land as `pending` on the next list
    // and flip when the reconcile finishes.
    let db = state.db.clone();
    let dir2 = dir.clone();
    tokio::spawn(async move {
        if let Err(e) = aichip_core::rag::index::reconcile(&db, project_id, &dir2).await {
            tracing::warn!(%project_id, error=%e, "space reconcile failed after upload");
        }
    });

    let docs = list_rows(&state, project_id).await?;
    Ok(Json(json!({ "stored": stored, "documents": docs })))
}

/// Awaited, unlike the background triggers: the button exists to answer
/// "did it work", so it returns the report.
async fn reindex(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let dir = space_path(&state, project_id).await?;
    let report = aichip_core::rag::index::reconcile(&state.db, project_id, &dir)
        .await
        .map_err(internal)?;
    Ok(Json(serde_json::to_value(report).map_err(internal)?))
}

async fn status(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    space_path(&state, project_id).await?;
    let rows = sqlx::query(
        "SELECT status, count(*) AS n FROM project_documents WHERE project_id=$1 GROUP BY status",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let mut counts = serde_json::Map::new();
    for r in &rows {
        counts.insert(r.get::<String, _>("status"), json!(r.get::<i64, _>("n")));
    }
    Ok(Json(json!({
        "embedder": aichip_core::rag::embed::status(),
        "counts": counts,
    })))
}

async fn remove(
    State(state): State<AppState>,
    Path((project_id, doc_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let dir = space_path(&state, project_id).await?;
    let rel: String =
        sqlx::query_scalar("SELECT rel_path FROM project_documents WHERE id=$1 AND project_id=$2")
            .bind(doc_id)
            .bind(project_id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?
            .ok_or((StatusCode::NOT_FOUND, "no such document".to_string()))?;

    // rel_path came from our own walk of the space folder, but a stored path
    // is still input: refuse anything that would resolve outside the space.
    if rel
        .split(['/', '\\'])
        .any(|part| part == ".." || part.is_empty())
    {
        return Err((StatusCode::BAD_REQUEST, "refusing a suspicious path".into()));
    }
    let target = dir.join(&rel);
    if target.exists() {
        tokio::fs::remove_file(&target).await.map_err(internal)?;
    }
    sqlx::query("DELETE FROM project_documents WHERE id=$1")
        .bind(doc_id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}
