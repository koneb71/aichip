//! Assigning a manager to a project, and reading what it did.
//!
//! A manager *is* a routine — kind `manage`, one per project, enforced by a
//! partial unique index — so everything about scheduling it already works:
//! the cron parser, the catch-up policy, the firing history, the notification
//! when it finishes. What these routes add is the project-shaped door onto it,
//! because "assign a manager to this project" is not a thing you go to a list
//! of workspace routines to do.
//!
//! The one endpoint with no counterpart in `routines` is the pass history.
//! A routine's history answers "did it run"; a manager's has to answer "what
//! did it do while I was asleep", which is `manager_actions`.

use super::{internal, ApiError};
use crate::AppState;
use aichip_core::manager;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local, Utc};
use croner::Cron;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/manager",
            get(read).put(upsert).delete(remove),
        )
        .route("/projects/{id}/manager/run", post(run_now))
        .route("/projects/{id}/manager/passes", get(passes))
}

/// The manager routine for a project, if it has one.
async fn manager_row(
    state: &AppState,
    project_id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, ApiError> {
    sqlx::query(
        "SELECT rt.id, rt.name, rt.prompt, rt.cron_expr, rt.catch_up, rt.enabled,
                rt.engine, rt.model_tier, rt.effort, rt.chat_id, rt.agent_id,
                rt.max_starts, a.name AS agent_name
           FROM routines rt
           LEFT JOIN agents a ON a.id = rt.agent_id
          WHERE rt.kind = 'manage' AND rt.project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)
}

/// Local, because that is the clock the scheduler evaluates routines against
/// — the same reason `routines::next_occurrences` does it.
fn next_at(expr: &str, enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }
    Cron::from_str(expr)
        .ok()?
        .find_next_occurrence(&Local::now(), false)
        .ok()
        .map(|t| t.to_rfc3339())
}

async fn read(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let Some(r) = manager_row(&state, project_id).await? else {
        return Ok(Json(json!({ "manager": Value::Null })));
    };
    let expr: String = r.get("cron_expr");
    let enabled: bool = r.get("enabled");
    Ok(Json(json!({
        "manager": {
            "id": r.get::<Uuid, _>("id"),
            "name": r.get::<String, _>("name"),
            "agentId": r.get::<Option<Uuid>, _>("agent_id"),
            "agentName": r.get::<Option<String>, _>("agent_name"),
            "brief": r.get::<String, _>("prompt"),
            "cronExpr": expr,
            "catchUp": r.get::<String, _>("catch_up"),
            "enabled": enabled,
            "engine": r.get::<Option<String>, _>("engine"),
            "modelTier": r.get::<Option<String>, _>("model_tier"),
            "effort": r.get::<Option<String>, _>("effort"),
            "chatId": r.get::<Option<Uuid>, _>("chat_id"),
            "maxStarts": manager::clamp_starts(r.get::<Option<i32>, _>("max_starts")),
            "nextAt": next_at(&expr, enabled),
        }
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManagerBody {
    agent_id: Option<Uuid>,
    /// What this project's manager should care about. May be empty — a
    /// manager with no brief still has a job.
    #[serde(default)]
    brief: String,
    cron_expr: String,
    #[serde(default)]
    catch_up: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    engine: Option<String>,
    model_tier: Option<String>,
    effort: Option<String>,
    /// Cards one pass may start. Clamped, never trusted.
    max_starts: Option<i32>,
}

async fn upsert(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<ManagerBody>,
) -> Result<Json<Value>, ApiError> {
    // Everything that can be wrong is wrong now, not at 9am tomorrow when
    // nobody is watching — the rule the routines editor already follows.
    if Cron::from_str(&body.cron_expr).is_err() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("\"{}\" isn't a valid schedule", body.cron_expr),
        ));
    }
    if let Some(c) = &body.catch_up {
        if !matches!(c.as_str(), "run_once" | "skip") {
            return Err((
                StatusCode::BAD_REQUEST,
                "catchUp must be run_once or skip".into(),
            ));
        }
    }
    if let Some(engine) = &body.engine {
        if state.orchestrator.engine(engine).is_none() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{engine} isn't installed on this machine"),
            ));
        }
    }

    // A repository, not a document space: a manager on a space would be
    // refused by every board tool it reached for. Refused here so the person
    // assigning it finds out at the click.
    let project = sqlx::query("SELECT workspace_id, kind, name FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    if project.get::<String, _>("kind") == "space" {
        return Err((
            StatusCode::BAD_REQUEST,
            "a document space has no board to manage".into(),
        ));
    }
    let workspace_id: Uuid = project.get("workspace_id");

    // The agent must be one this workspace actually has. Checked rather than
    // trusted, for the same reason `tasks::resolve_agent_by_name` scopes
    // through the project: a cross-workspace binding is refused by every
    // later edit, so accepting one here makes a manager that cannot be saved
    // again.
    if let Some(agent_id) = body.agent_id {
        let ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM agents WHERE id = $1 AND workspace_id = $2)",
        )
        .bind(agent_id)
        .bind(workspace_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?;
        if !ok {
            return Err((
                StatusCode::BAD_REQUEST,
                "that agent is not in this project's workspace".into(),
            ));
        }
    }

    let max_starts = manager::clamp_starts(body.max_starts);
    let name = format!("{} manager", project.get::<String, _>("name"));

    // Upsert on the partial unique index — one manager per project, and
    // re-assigning replaces rather than accumulating. `chat_id` is untouched
    // on update, deliberately: the standing thread is the manager's memory,
    // and changing the schedule or the brief should not amount to firing it
    // and hiring someone with amnesia.
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO routines
            (workspace_id, name, kind, project_id, prompt, cron_expr, catch_up,
             enabled, engine, model_tier, effort, agent_id, max_starts)
         VALUES ($1,$2,'manage',$3,$4,$5,coalesce($6,'run_once'),
                 coalesce($7,true),$8,$9,$10,$11,$12)
         ON CONFLICT (project_id) WHERE kind = 'manage'
         DO UPDATE SET name = EXCLUDED.name,
                       prompt = EXCLUDED.prompt,
                       cron_expr = EXCLUDED.cron_expr,
                       catch_up = EXCLUDED.catch_up,
                       enabled = EXCLUDED.enabled,
                       engine = EXCLUDED.engine,
                       model_tier = EXCLUDED.model_tier,
                       effort = EXCLUDED.effort,
                       agent_id = EXCLUDED.agent_id,
                       max_starts = EXCLUDED.max_starts,
                       updated_at = now()
         RETURNING id",
    )
    .bind(workspace_id)
    .bind(&name)
    .bind(project_id)
    .bind(body.brief.trim())
    .bind(&body.cron_expr)
    .bind(&body.catch_up)
    .bind(body.enabled)
    .bind(&body.engine)
    .bind(&body.model_tier)
    .bind(&body.effort)
    .bind(body.agent_id)
    .bind(max_starts)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    Ok(Json(json!({ "id": id })))
}

/// Unassign the manager.
///
/// Deletes the routine, which cascades its firing history and the actions
/// recorded against it. The cards it made are ordinary cards and stay — the
/// manager is being dismissed, not its work undone.
async fn remove(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    sqlx::query("DELETE FROM routines WHERE kind = 'manage' AND project_id = $1")
        .bind(project_id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "ok": true })))
}

/// Run a pass now, without waiting for the schedule.
async fn run_now(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = manager_row(&state, project_id).await?.ok_or((
        StatusCode::NOT_FOUND,
        "this project has no manager".to_string(),
    ))?;
    let id: Uuid = row.get("id");
    // `fire` records the firing either way, so a refusal — the thread is
    // busy, the engine is gone — lands in the history a person can read
    // rather than only in this response.
    aichip_core::routines::fire(&state.db, &state.orchestrator, id, "manual")
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// What the manager has done, pass by pass.
async fn passes(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT rr.id, rr.fired_at, rr.trigger, rr.error, rr.run_id,
                r.status AS run_status, r.cost_usd
           FROM routine_runs rr
           JOIN routines rt ON rt.id = rr.routine_id
           LEFT JOIN runs r ON r.id = rr.run_id
          WHERE rt.kind = 'manage' AND rt.project_id = $1
          ORDER BY rr.fired_at DESC
          LIMIT 20",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let ids: Vec<Uuid> = rows.iter().map(|r| r.get("id")).collect();
    // One query for every pass's actions rather than one per pass: twenty
    // passes is twenty round trips for a panel that polls.
    let action_rows = sqlx::query(
        "SELECT ma.routine_run_id, ma.kind, ma.task_id, ma.detail, ma.created_at,
                t.title AS task_title, t.board_column
           FROM manager_actions ma
           LEFT JOIN tasks t ON t.id = ma.task_id
          WHERE ma.routine_run_id = ANY($1)
          ORDER BY ma.created_at",
    )
    .bind(&ids)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let pass_id: Uuid = r.get("id");
            let actions: Vec<Value> = action_rows
                .iter()
                .filter(|a| a.get::<Uuid, _>("routine_run_id") == pass_id)
                .map(|a| {
                    json!({
                        "kind": a.get::<String, _>("kind"),
                        "taskId": a.get::<Option<Uuid>, _>("task_id"),
                        // The title as the card has it now, falling back to
                        // the one recorded at the time — which is the only
                        // one left once the card is deleted.
                        "title": a
                            .get::<Option<String>, _>("task_title")
                            .unwrap_or_else(|| a.get::<String, _>("detail")),
                        "detail": a.get::<String, _>("detail"),
                        "column": a.get::<Option<String>, _>("board_column"),
                    })
                })
                .collect();
            json!({
                "id": pass_id,
                "firedAt": r.get::<DateTime<Utc>, _>("fired_at").to_rfc3339(),
                "trigger": r.get::<String, _>("trigger"),
                "error": r.get::<Option<String>, _>("error"),
                "runId": r.get::<Option<Uuid>, _>("run_id"),
                "runStatus": r.get::<Option<String>, _>("run_status"),
                "costUsd": r.get::<Option<f64>, _>("cost_usd"),
                "actions": actions,
            })
        })
        .collect();
    Ok(Json(json!({ "passes": items })))
}
