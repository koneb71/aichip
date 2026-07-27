use super::{internal, ApiError};
use crate::AppState;
use aichip_shared::workflow::{self, Workflow};
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
        .route("/workflows", get(list).post(create))
        .route("/workflows/{id}", patch(update).delete(remove))
        .route("/workflows/{id}/run", post(run))
        .route("/projects/{id}/workflows/sync", post(sync_from_repo))
        .route("/runs/{id}/steps", get(run_steps))
        .route("/workflow-runs", get(list_runs))
        .route("/teams/{id}/run", post(run_team))
}

fn workflow_json(r: &sqlx::postgres::PgRow) -> Value {
    // A workflow that fails to parse still lists — the editor is where the
    // user fixes it, so surface the error instead of hiding the row.
    let yaml: String = r.get("source_yaml");
    let (steps, error) = match Workflow::from_yaml(&yaml) {
        Ok(wf) => (wf.steps.len(), None),
        Err(e) => (0, Some(e.to_string())),
    };
    json!({
        "id": r.get::<Uuid, _>("id"),
        "projectId": r.get::<Uuid, _>("project_id"),
        "name": r.get::<String, _>("name"),
        "description": r.get::<String, _>("description"),
        "sourceYaml": yaml,
        "cronExpr": r.get::<Option<String>, _>("cron_expr"),
        "enabled": r.get::<bool, _>("enabled"),
        "lastRunAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_run_at"),
        "stepCount": steps,
        "error": error,
    })
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
        "SELECT w.* FROM workflows w JOIN projects p ON p.id = w.project_id
         WHERE ($1::uuid IS NULL OR w.project_id = $1)
           AND ($2::uuid IS NULL OR p.workspace_id = $2)
         ORDER BY w.created_at DESC",
    )
    .bind(filter.project_id)
    .bind(filter.workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(
        json!({ "workflows": rows.iter().map(workflow_json).collect::<Vec<_>>() }),
    ))
}

#[derive(Deserialize)]
struct CreateWorkflow {
    project_id: Uuid,
    source_yaml: String,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateWorkflow>,
) -> Result<Json<Value>, ApiError> {
    let wf = Workflow::from_yaml(&body.source_yaml)
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    let row = upsert_workflow(&state, body.project_id, &wf, &body.source_yaml)
        .await
        .map_err(internal)?;
    Ok(Json(workflow_json(&row)))
}

async fn upsert_workflow(
    state: &AppState,
    project_id: Uuid,
    wf: &Workflow,
    yaml: &str,
) -> anyhow::Result<sqlx::postgres::PgRow> {
    let cron = wf.on.as_ref().and_then(|t| t.schedule.clone());
    let row = sqlx::query(
        "INSERT INTO workflows (project_id, name, description, kind, source_yaml, cron_expr)
         VALUES ($1,$2,$3,'pipeline',$4,$5)
         ON CONFLICT (project_id, name) DO UPDATE SET
             description = EXCLUDED.description,
             source_yaml = EXCLUDED.source_yaml,
             cron_expr = EXCLUDED.cron_expr
         RETURNING *",
    )
    .bind(project_id)
    .bind(&wf.name)
    .bind(&wf.description)
    .bind(yaml)
    .bind(cron)
    .fetch_one(&state.db.pool)
    .await?;
    Ok(row)
}

#[derive(Deserialize)]
struct UpdateWorkflow {
    source_yaml: Option<String>,
    enabled: Option<bool>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateWorkflow>,
) -> Result<Json<Value>, ApiError> {
    if let Some(yaml) = &body.source_yaml {
        Workflow::from_yaml(yaml)
            .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    }
    let row = sqlx::query(
        "UPDATE workflows SET source_yaml = COALESCE($1, source_yaml),
                enabled = COALESCE($2, enabled)
         WHERE id = $3 RETURNING *",
    )
    .bind(body.source_yaml)
    .bind(body.enabled)
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(workflow_json(&row)))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    sqlx::query("DELETE FROM workflows WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}

async fn run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let run_id = state
        .orchestrator
        .enqueue_workflow(id, "manual")
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "runId": run_id })))
}

/// Import every `.aichip/workflows/*.yaml` in the project's repo.
async fn sync_from_repo(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let path: String = sqlx::query("SELECT path FROM projects WHERE id=$1")
        .bind(project_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?
        .get("path");
    let dir = std::path::Path::new(&path).join(".aichip").join("workflows");

    let mut imported: Vec<Value> = vec![];
    let mut errors: Vec<Value> = vec![];
    let mut entries = match tokio::fs::read_dir(&dir).await {
        Ok(e) => e,
        Err(_) => {
            return Ok(Json(json!({
                "imported": [], "errors": [],
                "note": format!("no {} directory in this project yet", dir.display()),
            })))
        }
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let file = entry.path();
        if !matches!(
            file.extension().and_then(|e| e.to_str()),
            Some("yaml") | Some("yml")
        ) {
            continue;
        }
        let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Ok(yaml) = tokio::fs::read_to_string(&file).await else {
            errors.push(json!({ "file": name, "error": "could not read file" }));
            continue;
        };
        match Workflow::from_yaml(&yaml) {
            Ok(wf) => match upsert_workflow(&state, project_id, &wf, &yaml).await {
                Ok(row) => imported.push(workflow_json(&row)),
                Err(e) => errors.push(json!({ "file": name, "error": e.to_string() })),
            },
            Err(e) => errors.push(json!({ "file": name, "error": e.to_string() })),
        }
    }
    Ok(Json(json!({ "imported": imported, "errors": errors })))
}

/// Step-by-step status of a workflow run — drives the run graph.
async fn run_steps(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT id, step_key, status, output_text, started_at, finished_at
         FROM steps WHERE run_id=$1 ORDER BY started_at ASC",
    )
    .bind(run_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let steps: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "stepKey": r.get::<String, _>("step_key"),
                "status": r.get::<String, _>("status"),
                "output": r.get::<Option<String>, _>("output_text"),
                "startedAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("started_at"),
                "finishedAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("finished_at"),
            })
        })
        .collect();
    Ok(Json(json!({ "steps": steps })))
}

async fn list_runs(
    State(state): State<AppState>,
    Query(filter): Query<ProjectFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT r.id, r.status, r.trigger, r.cost_usd, r.created_at, r.error_reason,
                w.name AS workflow_name, w.id AS workflow_id
         FROM runs r JOIN workflows w ON w.id = r.workflow_id
         JOIN projects p ON p.id = w.project_id
         WHERE ($1::uuid IS NULL OR w.project_id = $1)
           AND ($2::uuid IS NULL OR p.workspace_id = $2)
         ORDER BY r.created_at DESC LIMIT 50",
    )
    .bind(filter.project_id)
    .bind(filter.workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let runs: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "workflowId": r.get::<Uuid, _>("workflow_id"),
                "workflowName": r.get::<String, _>("workflow_name"),
                "status": r.get::<String, _>("status"),
                "trigger": r.get::<String, _>("trigger"),
                "costUsd": r.get::<Option<f64>, _>("cost_usd"),
                "error": r.get::<Option<String>, _>("error_reason"),
                "createdAt": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        })
        .collect();
    Ok(Json(json!({ "runs": runs })))
}

#[derive(Deserialize)]
struct RunTeam {
    project_id: Uuid,
    goal: String,
}

/// Run a team: its pattern and members become a workflow, which then goes
/// through the ordinary pipeline executor.
async fn run_team(
    State(state): State<AppState>,
    Path(team_id): Path<Uuid>,
    Json(body): Json<RunTeam>,
) -> Result<Json<Value>, ApiError> {
    if body.goal.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "a goal is required".into()));
    }
    let team = sqlx::query("SELECT name, pattern, definition FROM teams WHERE id=$1")
        .bind(team_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?;

    let definition: Value = team.get("definition");
    let member_ids: Vec<Uuid> = definition
        .get("members")
        .and_then(Value::as_array)
        .map(|ms| {
            ms.iter()
                .filter_map(|m| m.get("agent_id").and_then(Value::as_str))
                .filter_map(|s| Uuid::parse_str(s).ok())
                .collect()
        })
        .unwrap_or_default();
    if member_ids.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "add at least one agent to this team first".into(),
        ));
    }

    // Preserve the author's member order, which the pattern depends on.
    let rows = sqlx::query("SELECT id, name FROM agents WHERE id = ANY($1)")
        .bind(&member_ids)
        .fetch_all(&state.db.pool)
        .await
        .map_err(internal)?;
    let names: Vec<String> = member_ids
        .iter()
        .filter_map(|id| {
            rows.iter()
                .find(|r| r.get::<Uuid, _>("id") == *id)
                .map(|r| r.get::<String, _>("name"))
        })
        .collect();

    let team_name: String = team.get("name");
    let pattern: String = team.get("pattern");
    let wf = workflow::from_team(&team_name, &pattern, &names, body.goal.trim());
    wf.validate()
        .map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, e.to_string()))?;
    let yaml = serde_yaml::to_string(&wf).map_err(internal)?;

    let row = upsert_workflow(&state, body.project_id, &wf, &yaml)
        .await
        .map_err(internal)?;
    let workflow_id: Uuid = row.get("id");
    let run_id = state
        .orchestrator
        .enqueue_workflow(workflow_id, "team")
        .await
        .map_err(internal)?;
    Ok(Json(
        json!({ "runId": run_id, "workflowId": workflow_id, "steps": wf.steps.len() }),
    ))
}
