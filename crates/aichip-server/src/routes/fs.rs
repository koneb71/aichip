//! Local folder browser for "load folder". The server runs on the user's own
//! machine; browsing is still sandboxed to $HOME and directory names only.

use super::{internal, ApiError};
use crate::AppState;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/fs/list", get(list))
        .route("/fs/git-init", post(git_init))
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Canonicalize `requested` and require it to live under `home`.
/// Returns None for anything that escapes (symlinks included).
fn sandboxed(home: &Path, requested: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(requested).ok()?;
    let home_canonical = std::fs::canonicalize(home).ok()?;
    canonical.starts_with(&home_canonical).then_some(canonical)
}

#[derive(Deserialize)]
struct ListQuery {
    path: Option<String>,
}

async fn list(Query(q): Query<ListQuery>) -> Result<Json<Value>, ApiError> {
    let home = home();
    let requested = q.path.map(PathBuf::from).unwrap_or_else(|| home.clone());
    let Some(path) = sandboxed(&home, &requested) else {
        return Err((StatusCode::FORBIDDEN, "path is outside your home directory".into()));
    };

    let mut dirs: Vec<Value> = vec![];
    let mut entries = tokio::fs::read_dir(&path).await.map_err(internal)?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let Ok(file_type) = entry.file_type().await else { continue };
        if !file_type.is_dir() {
            continue;
        }
        let dir_path = entry.path();
        dirs.push(json!({
            "name": name,
            "path": dir_path.to_string_lossy(),
            "isGitRepo": dir_path.join(".git").exists(),
        }));
    }
    dirs.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });

    let parent = path
        .parent()
        .and_then(|p| sandboxed(&home, p))
        .map(|p| p.to_string_lossy().into_owned());
    Ok(Json(json!({
        "path": path.to_string_lossy(),
        "parent": parent,
        "isGitRepo": path.join(".git").exists(),
        "dirs": dirs,
    })))
}

#[derive(Deserialize)]
struct GitInitBody {
    path: String,
}

async fn git_init(Json(body): Json<GitInitBody>) -> Result<Json<Value>, ApiError> {
    let home = home();
    let Some(path) = sandboxed(&home, Path::new(&body.path)) else {
        return Err((StatusCode::FORBIDDEN, "path is outside your home directory".into()));
    };
    if path.join(".git").exists() {
        return Err((StatusCode::BAD_REQUEST, "already a git repository".into()));
    }
    let out = tokio::process::Command::new("git")
        .current_dir(&path)
        .args(["init", "-b", "main"])
        .output()
        .await
        .map_err(internal)?;
    if !out.status.success() {
        return Err(internal(String::from_utf8_lossy(&out.stderr)));
    }
    Ok(Json(json!({ "initialized": true, "path": path.to_string_lossy() })))
}

#[cfg(test)]
mod tests {
    use super::sandboxed;
    use std::path::Path;

    #[test]
    fn sandbox_accepts_home_subdirs_and_rejects_escapes() {
        let home = std::env::var("HOME").unwrap();
        let home = Path::new(&home);
        // Home itself is allowed.
        assert!(sandboxed(home, home).is_some());
        // Traversal back into home is fine after canonicalization…
        assert!(sandboxed(home, &home.join("Documents/..")).is_some());
        // …but escaping is not.
        assert!(sandboxed(home, Path::new("/tmp")).is_none());
        assert!(sandboxed(home, &home.join("..")).is_none());
        // Nonexistent paths are rejected (canonicalize fails).
        assert!(sandboxed(home, &home.join("definitely-not-a-real-dir-xyz")).is_none());
    }
}
