use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
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
        .route("/org-runs/{id}/plan/approve", post(approve_plan))
        .route("/org-runs/{id}/plan/reject", post(reject_plan))
        .route(
            "/org-runs/{id}/assignments/{step_id}",
            patch(update_assignment).delete(drop_assignment),
        )
}

#[derive(Deserialize)]
struct RunOrg {
    project_id: Uuid,
    goal: String,
    /// Pause after planning so the plan can be edited before anyone starts.
    #[serde(default)]
    review_plan: bool,
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
        .enqueue_org_run(team_id, body.project_id, body.goal.trim(), body.review_plan)
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

/// Manager thinking, or work handed to someone? The UI used to hardcode
/// this and would leak repair and re-plan steps into the assignments panel.
fn step_kind(key: &str) -> &'static str {
    if matches!(key, "plan" | "plan_repair" | "review")
        || key.starts_with("replan/")
        || key.starts_with("triage/")
    {
        "manager"
    } else {
        "assignment"
    }
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
                done_when, size, origin, attempt, started_at, finished_at
         FROM steps WHERE run_id = $1
         ORDER BY position NULLS LAST, started_at NULLS LAST, id",
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
            "doneWhen": r.get::<Vec<String>, _>("done_when"),
            "size": r.get::<Option<String>, _>("size"),
            "origin": r.get::<String, _>("origin"),
            "attempt": r.get::<i32, _>("attempt"),
            "kind": step_kind(&r.get::<String, _>("step_key")),
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

// ── Plan review ────────────────────────────────────────────────────────────
//
// A parked run holds no concurrency permit and no engine process — the plan
// is just rows in `steps`. Approving re-queues it; the executor notices the
// plan already exists and goes straight to work.

/// Guard shared by every plan-editing route: only a parked run's queued
/// assignments may be touched.
async fn assert_parked(state: &AppState, run_id: Uuid) -> Result<(), ApiError> {
    let status: String = sqlx::query("SELECT status FROM runs WHERE id=$1")
        .bind(run_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such run".to_string()))?
        .get("status");
    if status != "awaiting_approval" {
        return Err((
            StatusCode::CONFLICT,
            format!("this run is {status}, so its plan is no longer editable"),
        ));
    }
    Ok(())
}

async fn approve_plan(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let updated = sqlx::query(
        "UPDATE runs SET plan_approved_at = now(), status = 'queued'
         WHERE id = $1 AND status = 'awaiting_approval'",
    )
    .bind(run_id)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            "this run is not waiting for approval".into(),
        ));
    }
    state
        .orchestrator
        .queue(run_id, 15)
        .await
        .map_err(internal)?;
    state
        .orchestrator
        .post(run_id, None, "system", None, "status", "Plan approved — starting work.")
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "approved": true })))
}

#[derive(Deserialize)]
struct Reject {
    #[serde(default)]
    reason: Option<String>,
}

async fn reject_plan(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(body): Json<Reject>,
) -> Result<Json<Value>, ApiError> {
    assert_parked(&state, run_id).await?;
    let reason = body
        .reason
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| "the plan was rejected".to_string());
    sqlx::query(
        "UPDATE steps SET status='skipped', finished_at=now()
         WHERE run_id=$1 AND status='queued'",
    )
    .bind(run_id)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    sqlx::query(
        "UPDATE runs SET status='canceled', error_reason=$2, finished_at=now() WHERE id=$1",
    )
    .bind(run_id)
    .bind(&reason)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    state
        .orchestrator
        .post(run_id, None, "system", None, "status", &format!("Run canceled — {reason}"))
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "canceled": true })))
}

#[derive(Deserialize)]
struct AssignmentPatch {
    title: Option<String>,
    brief: Option<String>,
    assignee: Option<String>,
    done_when: Option<Vec<String>>,
    position: Option<f64>,
}

async fn update_assignment(
    State(state): State<AppState>,
    Path((run_id, step_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AssignmentPatch>,
) -> Result<Json<Value>, ApiError> {
    assert_parked(&state, run_id).await?;

    // Reassigning to someone who isn't on this team would strand the work.
    if let Some(name) = body.assignee.as_deref() {
        let on_team: i64 = sqlx::query(
            "SELECT COUNT(*) AS n FROM agents a
             JOIN teams t ON t.workspace_id = a.workspace_id
             JOIN runs r ON r.team_id = t.id
             WHERE r.id = $1 AND a.name = $2
               AND t.definition->'members' @> jsonb_build_array(
                     jsonb_build_object('agent_id', a.id::text))",
        )
        .bind(run_id)
        .bind(name)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?
        .get("n");
        if on_team == 0 {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{name} is not a specialist on this team"),
            ));
        }
    }

    let updated = sqlx::query(
        "UPDATE steps SET title = COALESCE($3, title),
                brief = COALESCE($4, brief),
                assignee = COALESCE($5, assignee),
                done_when = COALESCE($6, done_when),
                position = COALESCE($7, position),
                origin = 'user'
         WHERE id = $2 AND run_id = $1 AND status = 'queued'",
    )
    .bind(run_id)
    .bind(step_id)
    .bind(body.title)
    .bind(body.brief)
    .bind(body.assignee)
    .bind(body.done_when)
    .bind(body.position)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            "that assignment has already started".into(),
        ));
    }
    Ok(Json(json!({ "updated": true })))
}

/// Soft delete: a dependent's `depends_on` still resolves against a skipped
/// key, and the audit trail survives.
async fn drop_assignment(
    State(state): State<AppState>,
    Path((run_id, step_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    assert_parked(&state, run_id).await?;
    sqlx::query(
        "UPDATE steps SET status='skipped', finished_at=now()
         WHERE id=$2 AND run_id=$1 AND status='queued'",
    )
    .bind(run_id)
    .bind(step_id)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "dropped": true })))
}
