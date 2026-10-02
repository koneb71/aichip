//! Checks: configuring a project's test and lint commands, and running them on
//! a card's worktree.
//!
//! **This file is the only writer of `project_checks`**, and a test below fails
//! if any other file in the workspace writes to it. The table holds shell
//! commands this machine runs, so who can write it is the whole security story:
//! a person, through the dashboard, with the write header no cross-origin page
//! can send — never an agent, an importer or an app build. The rest of the
//! reasoning is at the top of `eren_core::checks`.

use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use eren_core::checks::{self, Check};
use eren_core::runs::follow_up::FollowUp;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{id}/checks", get(get_config).put(put_config))
        .route("/tasks/{id}/checks", get(latest).post(run_now))
        .route("/tasks/{id}/checks/fix", post(fix))
}

/// See [`super::require_write`]: a check is a shell command this machine runs.
fn require_write_header(headers: &HeaderMap, what: &str) -> Result<(), ApiError> {
    super::require_write(headers, &format!("this endpoint {what}"))
}

async fn get_config(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT commands, timeout_secs, auto_fix_attempts FROM project_checks WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(match row {
        Some(r) => json!({
            "commands": r.get::<Value, _>("commands"),
            "timeoutSecs": r.get::<i32, _>("timeout_secs"),
            "autoFixAttempts": r.get::<i32, _>("auto_fix_attempts"),
        }),
        None => json!({ "commands": [], "timeoutSecs": 600, "autoFixAttempts": 0 }),
    }))
}

#[derive(Deserialize)]
pub(crate) struct ConfigBody {
    commands: Vec<Check>,
    timeout_secs: i32,
    auto_fix_attempts: i32,
}

pub(crate) async fn put_config(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<ConfigBody>,
) -> Result<Json<Value>, ApiError> {
    require_write_header(&headers, "stores commands this machine will run")?;
    let kind: String = sqlx::query_scalar("SELECT kind FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    // An app's changes land by themselves the moment they finish; there is no
    // review for checks to sit in front of.
    if kind != "repo" {
        return Err((
            StatusCode::BAD_REQUEST,
            "checks are for code repositories; an app's changes land on their own".into(),
        ));
    }
    let commands = checks::validate(&body.commands, body.timeout_secs, body.auto_fix_attempts)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    eren_core::revisions::keep(
        &state.db,
        eren_core::revisions::EntityKind::ProjectChecks,
        &project_id.to_string(),
    )
    .await;
    sqlx::query(
        "INSERT INTO project_checks (project_id, commands, timeout_secs, auto_fix_attempts, updated_at)
         VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (project_id) DO UPDATE
            SET commands = EXCLUDED.commands, timeout_secs = EXCLUDED.timeout_secs,
                auto_fix_attempts = EXCLUDED.auto_fix_attempts, updated_at = now()",
    )
    .bind(project_id)
    .bind(serde_json::to_value(&commands).map_err(internal)?)
    .bind(body.timeout_secs)
    .bind(body.auto_fix_attempts)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    get_config(State(state), Path(project_id)).await
}

/// The card's newest check run, in full — results, output and all.
async fn latest(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let configured: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM project_checks pc JOIN tasks t ON t.project_id = pc.project_id
                         WHERE t.id = $1 AND jsonb_array_length(pc.commands) > 0)",
    )
    .bind(task_id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let row = sqlx::query(
        "SELECT id, run_id, status, started_by, results, dirtied, error, created_at, finished_at
           FROM check_runs WHERE task_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    let latest = row.map(|r| {
        json!({
            "id": r.get::<Uuid, _>("id"),
            "runId": r.get::<Option<Uuid>, _>("run_id"),
            "status": r.get::<String, _>("status"),
            "startedBy": r.get::<String, _>("started_by"),
            "results": r.get::<Value, _>("results"),
            "dirtied": r.get::<Value, _>("dirtied"),
            "error": r.get::<Option<String>, _>("error"),
            "createdAt": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            "finishedAt": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("finished_at"),
        })
    });
    Ok(Json(json!({ "configured": configured, "latest": latest })))
}

/// A person asked for the checks. This click is the consent that a run in any
/// mode short of Full Auto never gave on its own.
async fn run_now(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_write_header(&headers, "runs this project's commands")?;
    let card = sqlx::query(
        "SELECT t.project_id, t.worktree_path,
                (SELECT id FROM runs WHERE task_id = t.id ORDER BY created_at DESC LIMIT 1) AS run_id,
                EXISTS (SELECT 1 FROM runs WHERE task_id = t.id
                         AND status NOT IN ('completed','failed','canceled')) AS live
           FROM tasks t WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such task".to_string()))?;
    // Checking half-written work tells nobody anything, and the agent is
    // still changing the tree the checks would read.
    if card.get::<bool, _>("live") {
        return Err((
            StatusCode::CONFLICT,
            "an agent is still working on this card — run the checks once it finishes".into(),
        ));
    }
    let dir = card
        .get::<Option<String>, _>("worktree_path")
        .filter(|w| std::path::Path::new(w).is_dir())
        .ok_or((
            StatusCode::CONFLICT,
            "this card has no worktree to check — it was merged, discarded, or never ran"
                .to_string(),
        ))?;
    let config = checks::config(&state.db, card.get("project_id"))
        .await
        .map_err(internal)?
        .ok_or((
            StatusCode::CONFLICT,
            "this project has no checks yet — add them in the project's settings".to_string(),
        ))?;
    let check_run_id = checks::begin(&state.db, task_id, card.get("run_id"), "person")
        .await
        .map_err(internal)?;
    let db = state.db.clone();
    tokio::spawn(async move {
        if let Err(e) =
            checks::execute(&db, check_run_id, std::path::Path::new(&dir), &config).await
        {
            tracing::warn!(%task_id, error = %e, "checks could not run");
            let _ = sqlx::query(
                "UPDATE check_runs SET status = 'error', error = $2, finished_at = now()
                  WHERE id = $1 AND status IN ('queued', 'running')",
            )
            .bind(check_run_id)
            .bind(e.to_string())
            .execute(&db.pool)
            .await;
        }
    });
    Ok(Json(json!({ "checkRunId": check_run_id })))
}

/// "Fix failing checks": a follow-up run in the card's worktree, briefed with
/// what failed.
async fn fix(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let check_run_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM check_runs WHERE task_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((
        StatusCode::CONFLICT,
        "these checks have not run yet".to_string(),
    ))?;
    let run_id = state
        .orchestrator
        .enqueue_follow_up(task_id, FollowUp::FailingChecks { check_run_id })
        .await
        .map_err(super::run_refused)?;
    Ok(Json(json!({ "runId": run_id })))
}

#[cfg(test)]
mod tests {
    /// Who can write `project_checks` is the whole security story of checks,
    /// so it is enforced here rather than remembered: any statement writing
    /// that table outside this file fails the build's tests. Modelled on
    /// `env_guard`'s rule for spawning processes.
    #[test]
    fn only_this_file_writes_project_checks() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/");
        let writes = [
            "INSERT INTO project_checks",
            "UPDATE project_checks",
            "DELETE FROM project_checks",
        ];
        let mut offenders = vec![];
        let mut stack = vec![crates.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("routes/checks.rs")
                {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                let flat = source.split_whitespace().collect::<Vec<_>>().join(" ");
                if writes.iter().any(|w| flat.contains(w)) {
                    offenders.push(path.display().to_string());
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "project_checks holds commands this machine runs; only routes/checks.rs may write it: {offenders:#?}"
        );
    }
}
