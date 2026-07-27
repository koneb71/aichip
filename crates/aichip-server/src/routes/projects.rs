use super::{internal, ApiError};
use crate::AppState;
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
        "SELECT id, path, name, default_branch, workspace_id FROM projects
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
    if !path.join(".git").exists() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{} is not a git repository", body.path),
        ));
    }
    let name = body.name.unwrap_or_else(|| {
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into())
    });
    let row = sqlx::query(
        "INSERT INTO projects (workspace_id, path, name, default_branch) VALUES ($1,$2,$3,$4)
         ON CONFLICT (path) DO UPDATE SET name = EXCLUDED.name, workspace_id = EXCLUDED.workspace_id
         RETURNING id",
    )
    .bind(body.workspace_id)
    .bind(&body.path)
    .bind(&name)
    .bind(body.default_branch.unwrap_or_else(|| "main".into()))
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "id": row.get::<Uuid, _>("id"), "name": name })))
}
