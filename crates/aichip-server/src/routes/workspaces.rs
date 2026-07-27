use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces", get(list).post(create))
        .route("/workspaces/{id}", patch(update).delete(remove))
}

async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query("SELECT id, name, icon, color FROM workspaces ORDER BY created_at ASC")
        .fetch_all(&state.db.pool)
        .await
        .map_err(internal)?;
    let workspaces: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "name": r.get::<String, _>("name"),
                "icon": r.get::<String, _>("icon"),
                "color": r.get::<String, _>("color"),
            })
        })
        .collect();
    Ok(Json(json!({ "workspaces": workspaces })))
}

#[derive(Deserialize)]
struct CreateWorkspace {
    name: String,
    color: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateWorkspace>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let row = sqlx::query(
        "INSERT INTO workspaces (name, color) VALUES ($1, $2) RETURNING id",
    )
    .bind(body.name.trim())
    .bind(body.color.unwrap_or_else(|| "#4f46e5".into()))
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })))
}

#[derive(Deserialize)]
struct UpdateWorkspace {
    name: Option<String>,
    color: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateWorkspace>,
) -> Result<Json<Value>, ApiError> {
    sqlx::query(
        "UPDATE workspaces SET name = COALESCE($1, name), color = COALESCE($2, color) WHERE id=$3",
    )
    .bind(body.name)
    .bind(body.color)
    .bind(id)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "updated": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM workspaces")
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?
        .get("n");
    if count <= 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "cannot delete the last workspace".into(),
        ));
    }
    sqlx::query("DELETE FROM workspaces WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}
