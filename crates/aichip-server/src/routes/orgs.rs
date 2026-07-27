use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/teams/{id}/run-org", post(run_org))
        .route("/org-runs", get(list))
        .route("/org-runs/{id}", get(detail))
}

#[derive(Deserialize)]
struct RunOrg {
    project_id: Uuid,
    goal: String,
}

async fn run_org(
    State(state): State<AppState>,
    Path(team_id): Path<Uuid>,
    Json(body): Json<RunOrg>,
) -> Result<Json<Value>, ApiError> {
    if body.goal.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "a goal is required".into()));
    }
    let team = sqlx::query("SELECT pattern, definition FROM teams WHERE id=$1")
        .bind(team_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?;
    if team.get::<String, _>("pattern") != "org" {
        return Err((
            StatusCode::BAD_REQUEST,
            "this team isn't an organization — set its pattern to Organization first".into(),
        ));
    }
    let definition: Value = team.get("definition");
    if definition.get("manager").and_then(Value::as_str).is_none() {
        return Err((StatusCode::BAD_REQUEST, "pick a manager for this organization".into()));
    }
    if definition
        .get("members")
        .and_then(Value::as_array)
        .is_none_or(|m| m.is_empty())
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "add at least one specialist for the manager to delegate to".into(),
        ));
    }

    let run_id = state
        .orchestrator
        .enqueue_org_run(team_id, body.project_id, body.goal.trim())
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "runId": run_id })))
}

#[derive(Deserialize)]
struct ProjectFilter {
    project_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<ProjectFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT r.id, r.status, r.goal, r.cost_usd, r.created_at, r.error_reason,
                t.name AS team_name, t.id AS team_id
         FROM runs r JOIN teams t ON t.id = r.team_id
         JOIN projects p ON p.id = r.project_id
         WHERE r.team_id IS NOT NULL
           AND ($1::uuid IS NULL OR r.project_id = $1)
           AND ($2::uuid IS NULL OR p.workspace_id = $2)
         ORDER BY r.created_at DESC LIMIT 30",
    )
    .bind(filter.project_id)
    .bind(filter.workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "runs": rows.iter().map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "teamId": r.get::<Uuid, _>("team_id"),
            "teamName": r.get::<String, _>("team_name"),
            "goal": r.get::<Option<String>, _>("goal"),
            "status": r.get::<String, _>("status"),
            "costUsd": r.get::<Option<f64>, _>("cost_usd"),
            "error": r.get::<Option<String>, _>("error_reason"),
            "createdAt": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        })).collect::<Vec<_>>()
    })))
}

/// Everything the live org view needs in one poll: the run, who's on the
/// team, what they were assigned, and what they've said.
async fn detail(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let run = sqlx::query(
        "SELECT r.id, r.status, r.goal, r.cost_usd, r.error_reason, r.created_at,
                t.id AS team_id, t.name AS team_name, t.definition, p.workspace_id
         FROM runs r JOIN teams t ON t.id = r.team_id
         JOIN projects p ON p.id = r.project_id
         WHERE r.id = $1",
    )
    .bind(run_id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(|_| (StatusCode::NOT_FOUND, "no such organization run".to_string()))?;

    let definition: Value = run.get("definition");
    let workspace_id: Uuid = run.get("workspace_id");

    // Roster: manager first, then specialists in the order they were added.
    let mut roster: Vec<Value> = vec![];
    if let Some(manager_id) = definition
        .get("manager")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
    {
        if let Some(agent) = load_agent(&state, workspace_id, manager_id).await? {
            roster.push(json!({
                "name": agent.0, "color": agent.1, "description": agent.2,
                "title": "Manager", "isManager": true,
            }));
        }
    }
    for entry in definition
        .get("members")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let Some(agent_id) = entry
            .get("agent_id")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            continue;
        };
        if let Some(agent) = load_agent(&state, workspace_id, agent_id).await? {
            roster.push(json!({
                "name": agent.0, "color": agent.1, "description": agent.2,
                "title": entry.get("role").and_then(Value::as_str).unwrap_or("Specialist"),
                "isManager": false,
            }));
        }
    }

    let assignments = sqlx::query(
        "SELECT id, step_key, status, assignee, title, brief, output_text, depends_on,
                started_at, finished_at
         FROM steps WHERE run_id = $1 ORDER BY started_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let messages = sqlx::query(
        "SELECT id, seq, from_agent, to_agent, kind, content, created_at
         FROM org_messages WHERE run_id = $1 ORDER BY seq ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({
        "id": run.get::<Uuid, _>("id"),
        "teamId": run.get::<Uuid, _>("team_id"),
        "teamName": run.get::<String, _>("team_name"),
        "goal": run.get::<Option<String>, _>("goal"),
        "status": run.get::<String, _>("status"),
        "costUsd": run.get::<Option<f64>, _>("cost_usd"),
        "error": run.get::<Option<String>, _>("error_reason"),
        "roster": roster,
        "assignments": assignments.iter().map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "key": r.get::<String, _>("step_key"),
            "status": r.get::<String, _>("status"),
            "assignee": r.get::<Option<String>, _>("assignee"),
            "title": r.get::<Option<String>, _>("title"),
            "brief": r.get::<Option<String>, _>("brief"),
            "output": r.get::<Option<String>, _>("output_text"),
            "dependsOn": r.get::<Vec<String>, _>("depends_on"),
            "startedAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("started_at"),
            "finishedAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("finished_at"),
        })).collect::<Vec<_>>(),
        "messages": messages.iter().map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "seq": r.get::<i64, _>("seq"),
            "from": r.get::<String, _>("from_agent"),
            "to": r.get::<Option<String>, _>("to_agent"),
            "kind": r.get::<String, _>("kind"),
            "content": r.get::<String, _>("content"),
            "ts": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        })).collect::<Vec<_>>(),
    })))
}

type AgentBits = (String, String, String);

async fn load_agent(
    state: &AppState,
    workspace_id: Uuid,
    agent_id: Uuid,
) -> Result<Option<AgentBits>, ApiError> {
    let row = sqlx::query(
        "SELECT name, color, description FROM agents WHERE id=$1 AND workspace_id=$2",
    )
    .bind(agent_id)
    .bind(workspace_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(row.map(|r| (r.get("name"), r.get("color"), r.get("description"))))
}
