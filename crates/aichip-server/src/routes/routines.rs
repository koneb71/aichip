//! Routines: a prompt that runs on a schedule.
//!
//! The list carries everything the page needs to be honest about the future
//! and the past: `nextAt` computed by the *same* croner that will fire it,
//! and the latest firings with links to what each one produced. The preview
//! endpoint exists for the editor — the next three occurrences shown while
//! typing come from the parser that decides, not a lookalike in JS.

use super::{internal, ApiError};
use crate::AppState;
use aichip_core::routines;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Local};
use croner::Cron;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces/{id}/routines", get(list).post(create))
        .route("/routines/preview", post(preview))
        .route("/routines/{id}", axum::routing::patch(update).delete(remove))
        .route("/routines/{id}/run", post(run_now))
        .route("/routines/{id}/runs", get(history))
}

/// Local time, because that is the clock the scheduler evaluates routines
/// against. Returning UTC here would make the editor's preview lie by a
/// timezone offset.
fn next_occurrences(expr: &str, n: usize) -> Vec<DateTime<Local>> {
    let Ok(cron) = Cron::from_str(expr) else { return vec![] };
    let mut out = Vec::with_capacity(n);
    let mut from = Local::now();
    for _ in 0..n {
        match cron.find_next_occurrence(&from, false) {
            Ok(t) => {
                out.push(t);
                from = t;
            }
            Err(_) => break,
        }
    }
    out
}

async fn list(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT rt.id, rt.name, rt.kind, rt.project_id, p.name AS project_name, rt.prompt,
                rt.cron_expr, rt.catch_up, rt.enabled, rt.engine, rt.model_tier, rt.effort,
                rt.chat_id, rt.created_at,
                lr.fired_at AS last_fired, lr.error AS last_error, lr.run_status AS last_status
         FROM routines rt
         LEFT JOIN projects p ON p.id = rt.project_id
         LEFT JOIN LATERAL (
             SELECT rr.fired_at, rr.error, r.status AS run_status
             FROM routine_runs rr LEFT JOIN runs r ON r.id = rr.run_id
             WHERE rr.routine_id = rt.id ORDER BY rr.fired_at DESC LIMIT 1
         ) lr ON TRUE
         WHERE rt.workspace_id = $1
         ORDER BY rt.created_at DESC",
    )
    .bind(workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let expr: String = r.get("cron_expr");
            json!({
                "id": r.get::<Uuid, _>("id"),
                "name": r.get::<String, _>("name"),
                "kind": r.get::<String, _>("kind"),
                "projectId": r.get::<Option<Uuid>, _>("project_id"),
                "projectName": r.get::<Option<String>, _>("project_name"),
                "prompt": r.get::<String, _>("prompt"),
                "cronExpr": expr,
                "catchUp": r.get::<String, _>("catch_up"),
                "enabled": r.get::<bool, _>("enabled"),
                "engine": r.get::<Option<String>, _>("engine"),
                "modelTier": r.get::<Option<String>, _>("model_tier"),
                "effort": r.get::<Option<String>, _>("effort"),
                "chatId": r.get::<Option<Uuid>, _>("chat_id"),
                // Only an enabled routine has a future.
                "nextAt": if r.get::<bool, _>("enabled") {
                    next_occurrences(&expr, 1).first().map(|t| t.to_rfc3339())
                } else { None },
                "lastFiredAt": r.get::<Option<DateTime<chrono::Utc>>, _>("last_fired").map(|t| t.to_rfc3339()),
                "lastError": r.get::<Option<String>, _>("last_error"),
                "lastRunStatus": r.get::<Option<String>, _>("last_status"),
            })
        })
        .collect();
    Ok(Json(json!({ "routines": items })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RoutineBody {
    name: String,
    kind: String,
    project_id: Option<Uuid>,
    prompt: String,
    cron_expr: String,
    #[serde(default)]
    catch_up: Option<String>,
    engine: Option<String>,
    model_tier: Option<String>,
    effort: Option<String>,
}

/// Everything that can be wrong with a routine is wrong at save time, not at
/// 9am tomorrow when nobody is watching — so the validation lives here.
fn vet(body: &RoutineBody) -> Result<(), ApiError> {
    if body.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "give the routine a name".into()));
    }
    if body.prompt.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "the prompt is empty".into()));
    }
    if !matches!(body.kind.as_str(), "chat" | "research" | "task") {
        return Err((StatusCode::BAD_REQUEST, "kind must be chat, research or task".into()));
    }
    if body.kind == "task" && body.project_id.is_none() {
        return Err((StatusCode::BAD_REQUEST, "a task routine needs a project board".into()));
    }
    if Cron::from_str(&body.cron_expr).is_err() {
        return Err((StatusCode::BAD_REQUEST, format!("\"{}\" isn't a valid schedule", body.cron_expr)));
    }
    if let Some(c) = &body.catch_up {
        if !matches!(c.as_str(), "run_once" | "skip") {
            return Err((StatusCode::BAD_REQUEST, "catchUp must be run_once or skip".into()));
        }
    }
    Ok(())
}

async fn create(
    State(state): State<AppState>,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<RoutineBody>,
) -> Result<Json<Value>, ApiError> {
    vet(&body)?;
    if let Some(engine) = &body.engine {
        if state.orchestrator.engine(engine).is_none() {
            return Err((StatusCode::BAD_REQUEST, format!("{engine} isn't installed on this machine")));
        }
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO routines (workspace_id, name, kind, project_id, prompt, cron_expr, catch_up, engine, model_tier, effort)
         VALUES ($1,$2,$3,$4,$5,$6,coalesce($7,'run_once'),$8,$9,$10) RETURNING id",
    )
    .bind(workspace_id)
    .bind(body.name.trim())
    .bind(&body.kind)
    .bind(body.project_id)
    .bind(body.prompt.trim())
    .bind(&body.cron_expr)
    .bind(&body.catch_up)
    .bind(&body.engine)
    .bind(&body.model_tier)
    .bind(&body.effort)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateBody {
    name: Option<String>,
    prompt: Option<String>,
    cron_expr: Option<String>,
    catch_up: Option<String>,
    enabled: Option<bool>,
    engine: Option<String>,
    model_tier: Option<String>,
    effort: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<Value>, ApiError> {
    if let Some(expr) = &body.cron_expr {
        if Cron::from_str(expr).is_err() {
            return Err((StatusCode::BAD_REQUEST, format!("\"{expr}\" isn't a valid schedule")));
        }
    }
    if let Some(c) = &body.catch_up {
        if !matches!(c.as_str(), "run_once" | "skip") {
            return Err((StatusCode::BAD_REQUEST, "catchUp must be run_once or skip".into()));
        }
    }
    // Re-enabling resets the bookmark: the scheduler measures the next
    // occurrence from now, instead of instantly "catching up" a window that
    // passed while the routine was off.
    let n = sqlx::query(
        "UPDATE routines SET
            name = coalesce($2, name),
            prompt = coalesce($3, prompt),
            cron_expr = coalesce($4, cron_expr),
            catch_up = coalesce($5, catch_up),
            engine = coalesce($6, engine),
            model_tier = coalesce($7, model_tier),
            effort = coalesce($8, effort),
            enabled = coalesce($9, enabled),
            last_fired_at = CASE WHEN $9 IS NOT DISTINCT FROM true AND NOT enabled
                                 THEN NULL ELSE last_fired_at END,
            updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.prompt.as_deref().map(str::trim))
    .bind(&body.cron_expr)
    .bind(&body.catch_up)
    .bind(&body.engine)
    .bind(&body.model_tier)
    .bind(&body.effort)
    .bind(body.enabled)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?
    .rows_affected();
    if n == 0 {
        return Err((StatusCode::NOT_FOUND, "no such routine".into()));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    // The standing chat thread and everything the routine produced stay:
    // deleting a schedule must not delete its answers.
    sqlx::query("DELETE FROM routines WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "ok": true })))
}

/// Fire now, even when disabled — "run now" is how you test a routine before
/// trusting it with a schedule. Does not touch the bookmark: a manual run
/// must not shift when the next scheduled one happens.
async fn run_now(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    routines::fire(&state.db, &state.orchestrator, id, "manual")
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewBody {
    cron_expr: String,
}

async fn preview(Json(body): Json<PreviewBody>) -> Result<Json<Value>, ApiError> {
    if Cron::from_str(&body.cron_expr).is_err() {
        return Ok(Json(json!({ "valid": false, "next": [] })));
    }
    let next: Vec<String> = next_occurrences(&body.cron_expr, 3)
        .into_iter()
        .map(|t| t.to_rfc3339())
        .collect();
    Ok(Json(json!({ "valid": true, "next": next })))
}

async fn history(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT rr.id, rr.fired_at, rr.trigger, rr.error,
                rr.run_id, rr.research_id, rr.task_id, rr.chat_id,
                r.status AS run_status, r.cost_usd,
                rs.title AS research_title, t.title AS task_title, t.project_id AS task_project
         FROM routine_runs rr
         LEFT JOIN runs r ON r.id = rr.run_id
         LEFT JOIN researches rs ON rs.id = rr.research_id
         LEFT JOIN tasks t ON t.id = rr.task_id
         WHERE rr.routine_id = $1
         ORDER BY rr.fired_at DESC LIMIT 20",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "firedAt": r.get::<DateTime<chrono::Utc>, _>("fired_at").to_rfc3339(),
                "trigger": r.get::<String, _>("trigger"),
                "error": r.get::<Option<String>, _>("error"),
                "runId": r.get::<Option<Uuid>, _>("run_id"),
                "runStatus": r.get::<Option<String>, _>("run_status"),
                "costUsd": r.get::<Option<f64>, _>("cost_usd"),
                "researchId": r.get::<Option<Uuid>, _>("research_id"),
                "researchTitle": r.get::<Option<String>, _>("research_title"),
                "taskId": r.get::<Option<Uuid>, _>("task_id"),
                "taskTitle": r.get::<Option<String>, _>("task_title"),
                "taskProjectId": r.get::<Option<Uuid>, _>("task_project"),
                "chatId": r.get::<Option<Uuid>, _>("chat_id"),
            })
        })
        .collect();
    Ok(Json(json!({ "runs": items })))
}
