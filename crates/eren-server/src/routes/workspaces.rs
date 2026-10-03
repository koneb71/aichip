use super::{internal, ApiError};
use crate::auth::Caller;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use eren_core::scope::Owned;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces", get(list).post(create))
        .route("/workspaces/{id}", patch(update).delete(remove))
}

async fn list(State(state): State<AppState>, caller: Caller) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, name, icon, color FROM workspaces
          WHERE $1::uuid IS NULL OR owner_id = $1
          ORDER BY created_at ASC",
    )
    .bind(caller.user_id())
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
    caller: Caller,
    Json(body): Json<CreateWorkspace>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let row = sqlx::query(
        "INSERT INTO workspaces (name, color, owner_id) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(body.name.trim())
    .bind(body.color.unwrap_or_else(|| "#4f46e5".into()))
    .bind(caller.user_id())
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
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateWorkspace>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Workspace(id)).await?;
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
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Workspace(id)).await?;
    // The last one *of this account's*: everyone keeps somewhere to land.
    let count: i64 =
        sqlx::query("SELECT COUNT(*) AS n FROM workspaces WHERE owner_id IS NOT DISTINCT FROM $1")
            .bind(caller.user_id())
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
