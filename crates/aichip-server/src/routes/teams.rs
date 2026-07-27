use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

const PATTERNS: &[&str] = &["pipeline", "debate", "swarm"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/teams", get(list).post(create))
        .route("/teams/{id}", patch(update).delete(remove))
}

fn team_json(r: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": r.get::<Uuid, _>("id"),
        "name": r.get::<String, _>("name"),
        "pattern": r.get::<String, _>("pattern"),
        "definition": r.get::<Value, _>("definition"),
    })
}

#[derive(Deserialize)]
struct WsFilter {
    workspace_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<WsFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT * FROM teams WHERE $1::uuid IS NULL OR workspace_id = $1 ORDER BY created_at ASC",
    )
    .bind(filter.workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "teams": rows.iter().map(team_json).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
struct TeamBody {
    workspace_id: Uuid,
    name: String,
    pattern: String,
    /// {"members": [{"agent_id": "...", "role": "..."}]}
    #[serde(default)]
    definition: Value,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<TeamBody>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name is required".into()));
    }
    if !PATTERNS.contains(&body.pattern.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("pattern must be one of {PATTERNS:?}"),
        ));
    }
    let row = sqlx::query(
        "INSERT INTO teams (workspace_id, name, pattern, definition)
         VALUES ($1,$2,$3,$4) RETURNING *",
    )
    .bind(body.workspace_id)
    .bind(body.name.trim())
    .bind(&body.pattern)
    .bind(&body.definition)
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(team_json(&row)))
}

#[derive(Deserialize)]
struct TeamPatch {
    name: Option<String>,
    pattern: Option<String>,
    definition: Option<Value>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<TeamPatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(p) = &body.pattern {
        if !PATTERNS.contains(&p.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("pattern must be one of {PATTERNS:?}"),
            ));
        }
    }
    let row = sqlx::query(
        "UPDATE teams SET name = COALESCE($1, name), pattern = COALESCE($2, pattern),
                definition = COALESCE($3, definition)
         WHERE id = $4 RETURNING *",
    )
    .bind(body.name)
    .bind(body.pattern)
    .bind(body.definition)
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(team_json(&row)))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    sqlx::query("DELETE FROM teams WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}
