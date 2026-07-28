//! Read-only file browser for a project checkout. This is a viewer, not an
//! editor: editing the user's working tree is what tasks (in isolated
//! worktrees) are for, so nothing here writes.
//!
//! `fs.rs` sandboxes to $HOME for "load folder"; this sandboxes to the
//! project's own root, which is stricter and independent of it.

use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{id}/files", get(list))
        .route("/projects/{id}/file", get(read_file))
}

/// Anything larger is reported as too-large rather than streamed into a
/// browser tab that would choke on it.
const MAX_FILE_BYTES: u64 = 512 * 1024;

/// Directories that are never useful in a source browser and are large
/// enough that walking them hurts.
const SKIP_DIRS: [&str; 3] = [".git", "node_modules", "target"];

/// Resolve a project-relative path inside `root`, rejecting anything that
/// escapes it. Canonicalizing both sides means `..` and symlinks that point
/// outside the checkout are caught, not just literal `../` in the string.
fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let root = std::fs::canonicalize(root).ok()?;
    let canonical = std::fs::canonicalize(root.join(rel.trim_start_matches('/'))).ok()?;
    canonical.starts_with(&root).then_some(canonical)
}

/// Path of `full` relative to `root`, using forward slashes — this is the
/// form the API speaks, so clients never see absolute host paths.
fn relative(root: &Path, full: &Path) -> String {
    std::fs::canonicalize(root)
        .ok()
        .and_then(|r| full.strip_prefix(&r).ok().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| full.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

async fn project_root(state: &AppState, id: Uuid) -> Result<PathBuf, ApiError> {
    let row = sqlx::query("SELECT path FROM projects WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    Ok(PathBuf::from(row.get::<String, _>("path")))
}

#[derive(Deserialize)]
struct PathQuery {
    /// Project-relative; absent or empty means the project root.
    path: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    UrlPath(project_id): UrlPath<Uuid>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, ApiError> {
    let root = project_root(&state, project_id).await?;
    let rel = q.path.unwrap_or_default();
    let Some(dir) = resolve(&root, &rel) else {
        return Err((StatusCode::FORBIDDEN, "path is outside the project".into()));
    };
    if !dir.is_dir() {
        return Err((StatusCode::BAD_REQUEST, "not a directory".into()));
    }

    let mut entries: Vec<Value> = vec![];
    let mut read = tokio::fs::read_dir(&dir).await.map_err(internal)?;
    while let Ok(Some(entry)) = read.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type().await else { continue };
        let is_dir = file_type.is_dir();
        if is_dir && SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let size = if is_dir {
            None
        } else {
            entry.metadata().await.ok().map(|m| m.len())
        };
        entries.push(json!({
            "name": name,
            "path": relative(&root, &entry.path()),
            "kind": if is_dir { "dir" } else { "file" },
            "size": size,
        }));
    }
    // Directories first, then case-insensitive by name.
    entries.sort_by(|a, b| {
        let key = |v: &Value| {
            (
                v["kind"].as_str().unwrap_or("") != "dir",
                v["name"].as_str().unwrap_or("").to_lowercase(),
            )
        };
        key(a).cmp(&key(b))
    });

    let rel_dir = relative(&root, &dir);
    let parent = if rel_dir.is_empty() {
        None
    } else {
        Some(
            Path::new(&rel_dir)
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default(),
        )
    };
    Ok(Json(json!({ "path": rel_dir, "parent": parent, "entries": entries })))
}

async fn read_file(
    State(state): State<AppState>,
    UrlPath(project_id): UrlPath<Uuid>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, ApiError> {
    let root = project_root(&state, project_id).await?;
    let rel = q.path.unwrap_or_default();
    if rel.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "path is required".into()));
    }
    let Some(file) = resolve(&root, &rel) else {
        return Err((StatusCode::FORBIDDEN, "path is outside the project".into()));
    };
    let meta = tokio::fs::metadata(&file).await.map_err(internal)?;
    if meta.is_dir() {
        return Err((StatusCode::BAD_REQUEST, "that is a directory".into()));
    }
    let rel_path = relative(&root, &file);
    if meta.len() > MAX_FILE_BYTES {
        return Ok(Json(json!({
            "path": rel_path, "size": meta.len(), "tooLarge": true,
            "binary": false, "content": Value::Null,
        })));
    }

    let bytes = tokio::fs::read(&file).await.map_err(internal)?;
    // A NUL byte is the same heuristic git uses to call a blob binary.
    let binary = bytes.contains(&0) || String::from_utf8(bytes.clone()).is_err();
    Ok(Json(json!({
        "path": rel_path,
        "size": meta.len(),
        "tooLarge": false,
        "binary": binary,
        "content": if binary { Value::Null } else { json!(String::from_utf8_lossy(&bytes)) },
    })))
}

#[cfg(test)]
mod tests {
    use super::{relative, resolve};
    use std::path::Path;

    #[test]
    fn resolve_keeps_paths_inside_the_project() {
        let root = std::env::temp_dir().join("aichip-files-test");
        let nested = root.join("src");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("main.rs"), "fn main() {}").unwrap();

        // Empty path is the root itself.
        assert_eq!(resolve(&root, "").unwrap(), std::fs::canonicalize(&root).unwrap());
        assert!(resolve(&root, "src/main.rs").is_some());
        // A leading slash is treated as project-relative, not absolute.
        assert!(resolve(&root, "/src/main.rs").is_some());
        // Traversal that lands back inside is fine…
        assert!(resolve(&root, "src/../src").is_some());
        // …but escaping the project is not.
        assert!(resolve(&root, "..").is_none());
        assert!(resolve(&root, "../../etc/passwd").is_none());
        // Nonexistent paths are rejected (canonicalize fails).
        assert!(resolve(&root, "src/nope.rs").is_none());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn relative_strips_the_root_and_uses_forward_slashes() {
        let root = std::env::temp_dir().join("aichip-files-rel");
        std::fs::create_dir_all(root.join("a")).unwrap();
        let canonical = std::fs::canonicalize(&root).unwrap();
        assert_eq!(relative(&root, &canonical), "");
        assert_eq!(relative(&root, &canonical.join("a")), "a");
        assert_eq!(relative(&root, Path::new(&canonical).join("a/b").as_path()), "a/b");
        std::fs::remove_dir_all(&root).ok();
    }
}
