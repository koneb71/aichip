//! Global search across a workspace — the thing the sidebar box now drives.
//!
//! Everything is scoped to one workspace: projects/agents/teams carry
//! `workspace_id` directly, tasks and workflows reach it through their
//! project. Results are grouped by kind and capped, so this stays a
//! jump-to-thing palette rather than a report.

use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/search", get(search))
}

/// Per-kind cap. Small on purpose: the palette is for jumping, and a long
/// list is worse than a short one plus a refined query.
const LIMIT: i64 = 6;

/// Build a case-insensitive "contains" pattern, neutralising the wildcards
/// a user can type. Without this, a query of `%` matches everything and `_`
/// silently matches any character.
fn contains(q: &str) -> String {
    let escaped = q
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    workspace_id: Uuid,
}

async fn search(
    State(state): State<AppState>,
    Query(sq): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let q = sq.q.trim();
    // One character matches most of the workspace; make the client's
    // debounce cheap by refusing to do the work at all.
    if q.len() < 2 {
        return Ok(Json(json!({
            "projects": [], "tasks": [], "agents": [], "teams": [], "workflows": [],
        })));
    }
    let pattern = contains(q);
    let pool = &state.db.pool;

    let projects = sqlx::query(
        "SELECT id, name, path FROM projects
         WHERE kind = 'repo' AND workspace_id=$1
           AND (name ILIKE $2 ESCAPE '\\' OR path ILIKE $2 ESCAPE '\\')
         ORDER BY created_at DESC LIMIT $3",
    )
    .bind(sq.workspace_id)
    .bind(&pattern)
    .bind(LIMIT)
    .fetch_all(pool)
    .await
    .map_err(internal)?
    .iter()
    .map(|r| {
        json!({
            "id": r.get::<Uuid, _>("id"),
            "label": r.get::<String, _>("name"),
            "sublabel": r.get::<String, _>("path"),
        })
    })
    .collect::<Vec<_>>();

    let tasks = sqlx::query(
        "SELECT t.id, t.title, t.board_column, t.project_id, p.name AS project_name
         FROM tasks t JOIN projects p ON p.id = t.project_id
         WHERE p.workspace_id=$1 AND t.title ILIKE $2 ESCAPE '\\'
         ORDER BY t.created_at DESC LIMIT $3",
    )
    .bind(sq.workspace_id)
    .bind(&pattern)
    .bind(LIMIT)
    .fetch_all(pool)
    .await
    .map_err(internal)?
    .iter()
    .map(|r| {
        json!({
            "id": r.get::<Uuid, _>("id"),
            "label": r.get::<String, _>("title"),
            "sublabel": format!(
                "{} · {}",
                r.get::<String, _>("project_name"),
                r.get::<String, _>("board_column"),
            ),
            "projectId": r.get::<Uuid, _>("project_id"),
        })
    })
    .collect::<Vec<_>>();

    let agents = sqlx::query(
        "SELECT id, name, description FROM agents
         WHERE workspace_id=$1 AND (name ILIKE $2 ESCAPE '\\' OR description ILIKE $2 ESCAPE '\\')
         ORDER BY name LIMIT $3",
    )
    .bind(sq.workspace_id)
    .bind(&pattern)
    .bind(LIMIT)
    .fetch_all(pool)
    .await
    .map_err(internal)?
    .iter()
    .map(|r| {
        json!({
            "id": r.get::<Uuid, _>("id"),
            "label": r.get::<String, _>("name"),
            "sublabel": r.get::<String, _>("description"),
        })
    })
    .collect::<Vec<_>>();

    let teams = sqlx::query(
        "SELECT id, name, pattern FROM teams
         WHERE workspace_id=$1 AND name ILIKE $2 ESCAPE '\\'
         ORDER BY name LIMIT $3",
    )
    .bind(sq.workspace_id)
    .bind(&pattern)
    .bind(LIMIT)
    .fetch_all(pool)
    .await
    .map_err(internal)?
    .iter()
    .map(|r| {
        json!({
            "id": r.get::<Uuid, _>("id"),
            "label": r.get::<String, _>("name"),
            "sublabel": r.get::<String, _>("pattern"),
        })
    })
    .collect::<Vec<_>>();

    let workflows = sqlx::query(
        "SELECT w.id, w.name, w.project_id, p.name AS project_name
         FROM workflows w JOIN projects p ON p.id = w.project_id
         WHERE p.workspace_id=$1
           AND (w.name ILIKE $2 ESCAPE '\\' OR w.description ILIKE $2 ESCAPE '\\')
         ORDER BY w.name LIMIT $3",
    )
    .bind(sq.workspace_id)
    .bind(&pattern)
    .bind(LIMIT)
    .fetch_all(pool)
    .await
    .map_err(internal)?
    .iter()
    .map(|r| {
        json!({
            "id": r.get::<Uuid, _>("id"),
            "label": r.get::<String, _>("name"),
            "sublabel": r.get::<String, _>("project_name"),
            "projectId": r.get::<Uuid, _>("project_id"),
        })
    })
    .collect::<Vec<_>>();

    Ok(Json(json!({
        "projects": projects,
        "tasks": tasks,
        "agents": agents,
        "teams": teams,
        "workflows": workflows,
    })))
}

#[cfg(test)]
mod tests {
    use super::contains;

    #[test]
    fn wildcards_typed_by_the_user_are_neutralised() {
        assert_eq!(contains("login"), "%login%");
        // `%` and `_` would otherwise match everything / any character.
        assert_eq!(contains("50%"), "%50\\%%");
        assert_eq!(contains("a_b"), "%a\\_b%");
        // Backslash is escaped first, so it can't smuggle in an escape.
        assert_eq!(contains("a\\b"), "%a\\\\b%");
    }
}
