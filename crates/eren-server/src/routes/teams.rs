use super::{internal, ApiError};
use crate::auth::Caller;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use eren_core::scope::Owned;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

const PATTERNS: &[&str] = &["pipeline", "debate", "swarm", "org"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/teams", get(list).post(create))
        .route("/teams/{id}", patch(patch_team).delete(remove))
        .route("/teams/{id}/estimate", get(estimate))
}

/// What this team has historically cost per run.
///
/// An org run can quietly turn into $15 and forty minutes, and the launch
/// modal offered no hint of that. Median rather than mean because one
/// runaway run shouldn't set the expectation for the next ten.
async fn estimate(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Team(id)).await?;
    let row = sqlx::query(
        "SELECT COUNT(*) AS runs,
                percentile_cont(0.5) WITHIN GROUP (ORDER BY cost_usd) AS median,
                MAX(cost_usd) AS worst,
                percentile_cont(0.5) WITHIN GROUP (
                    ORDER BY EXTRACT(EPOCH FROM (finished_at - started_at))) AS median_secs
         FROM runs
         WHERE team_id = $1 AND status = 'completed' AND cost_usd IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({
        "runs": row.get::<i64, _>("runs"),
        "medianUsd": row.get::<Option<f64>, _>("median"),
        "worstUsd": row.get::<Option<f64>, _>("worst"),
        "medianSecs": row.get::<Option<f64>, _>("median_secs"),
    })))
}

fn team_json(r: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": r.get::<Uuid, _>("id"),
        "name": r.get::<String, _>("name"),
        "pattern": r.get::<String, _>("pattern"),
        "definition": r.get::<Value, _>("definition"),
        // Null means "whatever the card or machine default says".
        "engine": r.get::<Option<String>, _>("engine"),
    })
}

#[derive(Deserialize)]
struct WsFilter {
    workspace_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Query(filter): Query<WsFilter>,
) -> Result<Json<Value>, ApiError> {
    let workspaces = caller.workspace_filter(&state, filter.workspace_id).await?;
    let rows = sqlx::query(
        "SELECT * FROM teams WHERE $1::uuid[] IS NULL OR workspace_id = ANY($1)
          ORDER BY created_at ASC",
    )
    .bind(workspaces)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(
        json!({ "teams": rows.iter().map(team_json).collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
struct TeamBody {
    workspace_id: Uuid,
    name: String,
    pattern: String,
    /// {"members": [{"agent_id": "...", "role": "..."}]}
    #[serde(default)]
    definition: Value,
    /// Which CLI this team runs on. `None` inherits from the card.
    #[serde(default)]
    engine: Option<String>,
}

/// Every agent a definition names must be the caller's: a team runs its
/// members, so naming someone else's agent would put it to work.
async fn require_members(
    caller: &Caller,
    state: &AppState,
    definition: &Value,
) -> Result<(), ApiError> {
    for agent in eren_core::agents::team_agent_ids(definition) {
        caller.require(state, Owned::Agent(agent)).await?;
    }
    Ok(())
}

async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<TeamBody>,
) -> Result<Json<Value>, ApiError> {
    caller
        .require(&state, Owned::Workspace(body.workspace_id))
        .await?;
    require_members(&caller, &state, &body.definition).await?;
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
        "INSERT INTO teams (workspace_id, name, pattern, definition, engine)
         VALUES ($1,$2,$3,$4,$5) RETURNING *",
    )
    .bind(body.workspace_id)
    .bind(body.name.trim())
    .bind(&body.pattern)
    .bind(&body.definition)
    .bind(body.engine.as_deref().filter(|e| !e.is_empty()))
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(team_json(&row)))
}

#[derive(Deserialize)]
pub(crate) struct TeamPatch {
    name: Option<String>,
    pattern: Option<String>,
    definition: Option<Value>,
    /// Present-but-null clears it back to inheriting.
    #[serde(default, deserialize_with = "double_option")]
    engine: Option<Option<String>>,
}

/// Distinguish "field absent" from "field set to null" so clearing works.
fn double_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(de).map(Some)
}

/// The route's door to [`update`], which a revision restore also calls once it
/// has checked the team itself.
async fn patch_team(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<TeamPatch>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Team(id)).await?;
    if let Some(definition) = &body.definition {
        require_members(&caller, &state, definition).await?;
    }
    update(State(state), Path(id), Json(body)).await
}

pub(crate) async fn update(
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
    eren_core::revisions::keep(
        &state.db,
        eren_core::revisions::EntityKind::Team,
        &id.to_string(),
    )
    .await;
    let row = sqlx::query(
        "UPDATE teams SET name = COALESCE($1, name), pattern = COALESCE($2, pattern),
                definition = COALESCE($3, definition),
                engine = CASE WHEN $6 THEN $5 ELSE engine END
         WHERE id = $4 RETURNING *",
    )
    .bind(body.name)
    .bind(body.pattern)
    .bind(body.definition)
    .bind(id)
    .bind(body.engine.clone().flatten().filter(|e| !e.is_empty()))
    .bind(body.engine.is_some())
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(team_json(&row)))
}

async fn remove(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Team(id)).await?;
    eren_core::revisions::keep(
        &state.db,
        eren_core::revisions::EntityKind::Team,
        &id.to_string(),
    )
    .await;
    sqlx::query("DELETE FROM teams WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}
