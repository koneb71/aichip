use super::{internal, ApiError};
use crate::AppState;
use aichip_core::worktrees::manager::{ensure_repo_state, Vcs};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/projects", get(list).post(create))
}

#[derive(Deserialize)]
pub struct WorkspaceFilter {
    pub workspace_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<WorkspaceFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, path, name, default_branch, workspace_id, vcs, vcs_note FROM projects
         WHERE $1::uuid IS NULL OR workspace_id = $1
         ORDER BY created_at DESC",
    )
    .bind(filter.workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let projects: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "path": r.get::<String, _>("path"),
                "name": r.get::<String, _>("name"),
                "defaultBranch": r.get::<String, _>("default_branch"),
                "workspaceId": r.get::<Uuid, _>("workspace_id"),
                "vcs": r.get::<String, _>("vcs"),
                "vcsNote": r.get::<Option<String>, _>("vcs_note"),
            })
        })
        .collect();
    Ok(Json(json!({ "projects": projects })))
}

#[derive(Deserialize)]
struct CreateProject {
    workspace_id: Uuid,
    path: String,
    name: Option<String>,
    default_branch: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateProject>,
) -> Result<Json<Value>, ApiError> {
    let path = std::path::Path::new(&body.path);
    if !path.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{} is not a directory", body.path),
        ));
    }
    let default_branch = body.default_branch.unwrap_or_else(|| "main".into());

    // Initialize a repository if this folder needs one. A worktree is what
    // keeps an agent out of the user's files and what makes review possible,
    // so it's worth creating a repo to get one — but a folder that can't have
    // one still becomes a project, with its runs happening in place.
    // Record the branch the repository is actually on. Assuming "main" for a
    // repo that uses something else makes every later `worktree add` fail.
    let repo = ensure_repo_state(path, &default_branch).await;
    let default_branch = repo.branch;
    let (vcs, vcs_note) = match repo.vcs {
        Vcs::Git => ("git", None),
        Vcs::None(reason) => ("none", Some(reason)),
    };

    let name = body.name.unwrap_or_else(|| {
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into())
    });
    let row = sqlx::query(
        "INSERT INTO projects (workspace_id, path, name, default_branch, vcs, vcs_note)
         VALUES ($1,$2,$3,$4,$5,$6)
         ON CONFLICT (path) DO UPDATE SET name = EXCLUDED.name,
             workspace_id = EXCLUDED.workspace_id,
             vcs = EXCLUDED.vcs, vcs_note = EXCLUDED.vcs_note
         RETURNING id",
    )
    .bind(body.workspace_id)
    .bind(&body.path)
    .bind(&name)
    .bind(&default_branch)
    .bind(vcs)
    .bind(&vcs_note)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "id": row.get::<Uuid, _>("id"),
        "name": name,
        "vcs": vcs,
        "vcsNote": vcs_note,
    })))
}
