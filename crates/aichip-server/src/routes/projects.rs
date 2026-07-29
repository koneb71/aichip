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
    Router::new()
        .route("/projects", get(list).post(create))
        .route("/projects/{id}", axum::routing::patch(update))
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
        "SELECT id, path, name, default_branch, workspace_id, vcs, vcs_note, full_auto_opt_in
         FROM projects
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
                "fullAutoOptIn": r.get::<bool, _>("full_auto_opt_in"),
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

#[derive(Deserialize)]
struct UpdateProject {
    /// Let agents work in this project without stopping to ask.
    ///
    /// This is the switch the FullAuto gate has always read and nothing has
    /// ever been able to set — which is why every run prompted for every
    /// action with no way to turn it off.
    full_auto_opt_in: Option<bool>,
    default_branch: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(body): Json<UpdateProject>,
) -> Result<Json<Value>, ApiError> {
    // Working without prompts is only safe because the run happens in an
    // aichip-managed worktree you review before merging. A project with no
    // version control edits your files directly, so there is nothing to
    // review and nothing to roll back — refuse rather than quietly downgrade
    // it later and leave the toggle looking enabled.
    if body.full_auto_opt_in == Some(true) {
        let vcs: String = sqlx::query_scalar("SELECT vcs FROM projects WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?
            .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
        if vcs != "git" {
            return Err((
                StatusCode::BAD_REQUEST,
                "this project has no version control, so runs edit your files directly —                  there is no isolated worktree to review or roll back. Working without                  prompts needs a git project."
                    .into(),
            ));
        }
    }

    let row = sqlx::query(
        "UPDATE projects
            SET full_auto_opt_in = COALESCE($2, full_auto_opt_in),
                default_branch   = COALESCE($3, default_branch)
          WHERE id = $1
      RETURNING id, path, name, default_branch, workspace_id, vcs, vcs_note, full_auto_opt_in",
    )
    .bind(id)
    .bind(body.full_auto_opt_in)
    .bind(body.default_branch.as_deref())
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;

    Ok(Json(json!({
        "id": row.get::<Uuid, _>("id"),
        "path": row.get::<String, _>("path"),
        "name": row.get::<String, _>("name"),
        "defaultBranch": row.get::<String, _>("default_branch"),
        "workspaceId": row.get::<Uuid, _>("workspace_id"),
        "vcs": row.get::<String, _>("vcs"),
        "vcsNote": row.get::<Option<String>, _>("vcs_note"),
        "fullAutoOptIn": row.get::<bool, _>("full_auto_opt_in"),
    })))
}
