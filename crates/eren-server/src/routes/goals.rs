//! Goals: the tree, one goal's page, and pointing cards at them.

use super::{internal, ApiError};
use crate::auth::Caller;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use eren_core::goals::{self, GoalPatch, NewGoal, Refused};
use eren_core::scope::Owned;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/goals", get(list).post(create))
        .route("/goals/{id}", get(detail).patch(update).delete(remove))
}

fn refused(r: Refused) -> ApiError {
    match r {
        Refused::Missing => (StatusCode::NOT_FOUND, r.to_string()),
        Refused::Invalid(m) => (StatusCode::BAD_REQUEST, m),
        other => (StatusCode::CONFLICT, other.to_string()),
    }
}

async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Path(workspace): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Workspace(workspace)).await?;
    let goals = goals::list(&state.db, workspace).await.map_err(internal)?;
    Ok(Json(
        json!({ "goals": goals, "maxDepth": goals::MAX_DEPTH }),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateBody {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    parent_id: Option<Uuid>,
    #[serde(default)]
    target_date: Option<NaiveDate>,
}

async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Path(workspace): Path<Uuid>,
    Json(body): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Workspace(workspace)).await?;
    caller
        .require_opt(&state, body.parent_id, Owned::Goal)
        .await?;
    let id = goals::create(
        &state.db,
        workspace,
        NewGoal {
            parent_id: body.parent_id,
            title: body.title,
            description: body.description,
            target_date: body.target_date,
        },
    )
    .await
    .map_err(internal)?
    .map_err(refused)?;
    Ok(Json(json!({ "id": id })))
}

/// Distinguish "field absent" from "field set to null".
fn present<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default, deserialize_with = "present")]
    parent_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present")]
    target_date: Option<Option<NaiveDate>>,
}

async fn update(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchBody>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Goal(id)).await?;
    caller
        .require_opt(&state, body.parent_id.flatten(), Owned::Goal)
        .await?;
    goals::update(
        &state.db,
        id,
        GoalPatch {
            parent_id: body.parent_id,
            title: body.title,
            description: body.description,
            status: body.status,
            target_date: body.target_date,
        },
    )
    .await
    .map_err(internal)?
    .map_err(refused)?;
    Ok(Json(json!({ "updated": true })))
}

/// Delete a goal; its children and cards move up to its parent.
async fn remove(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Goal(id)).await?;
    if !goals::delete(&state.db, id).await.map_err(internal)? {
        return Err((StatusCode::NOT_FOUND, "no such goal".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

/// One goal's page: where it sits, the cards that serve it (its subtree's),
/// the projects they are in, and what it has cost.
async fn detail(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    caller.require(&state, Owned::Goal(id)).await?;
    let (_, goal) = goals::get(&state.db, id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such goal".to_string()))?;
    let under = goals::subtree(&state.db, id).await.map_err(internal)?;
    let chain = goals::chain(&state.db, id).await.map_err(internal)?;
    let cards = sqlx::query(
        "SELECT t.id, t.title, t.board_column, t.project_id, p.name AS project, t.goal_id,
                a.name AS agent
           FROM tasks t JOIN projects p ON p.id = t.project_id
           LEFT JOIN agents a ON a.id = t.agent_id
          WHERE t.goal_id = ANY($1)
          ORDER BY (t.board_column = 'done'), t.created_at DESC LIMIT 200",
    )
    .bind(&under)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let spend: (f64, i64) = sqlx::query_as(
        "SELECT COALESCE(sum(r.cost_usd), 0)::float8, count(*) FROM runs r
           JOIN tasks t ON t.id = r.task_id WHERE t.goal_id = ANY($1)",
    )
    .bind(&under)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let mut projects: Vec<Value> = vec![];
    for r in &cards {
        let pid: Uuid = r.get("project_id");
        if !projects.iter().any(|p| p["id"] == json!(pid)) {
            projects.push(json!({ "id": pid, "name": r.get::<String, _>("project") }));
        }
    }
    Ok(Json(json!({
        "goal": goal,
        "chain": chain.iter().map(|(t, _)| t).collect::<Vec<_>>(),
        "cards": cards.iter().map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "title": r.get::<String, _>("title"),
            "column": r.get::<String, _>("board_column"),
            "projectId": r.get::<Uuid, _>("project_id"),
            "agent": r.get::<Option<String>, _>("agent"),
            "direct": r.get::<Option<Uuid>, _>("goal_id") == Some(id),
        })).collect::<Vec<_>>(),
        "projects": projects,
        "spendUsd": spend.0,
        "runs": spend.1,
    })))
}

/// A card may point only at a goal of its own workspace.
pub(crate) async fn vet_card_goal(
    state: &AppState,
    task_id: Uuid,
    goal: Uuid,
) -> Result<(), ApiError> {
    let same: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tasks t JOIN projects p ON p.id = t.project_id
                          JOIN goals g ON g.workspace_id = p.workspace_id
                         WHERE t.id = $1 AND g.id = $2)",
    )
    .bind(task_id)
    .bind(goal)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    if same {
        Ok(())
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            "that goal is not in this card's workspace".into(),
        ))
    }
}
