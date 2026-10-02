//! A project's review policy, and a card's reviews.
//!
//! **This file is the only writer of `project_review_policy`**, and a test
//! below fails the build otherwise. The policy decides what a person's Merge
//! requires and whether an agent reviews each run unasked — so, like a check
//! command, it is a person's setting: written only with the dashboard's write
//! header, and never from anything an agent can call.

use super::{internal, require_write, run_refused, ApiError};
use crate::AppState;
use aichip_core::review::{self, Policy, Start};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{id}/review-policy",
            get(get_policy).put(put_policy),
        )
        .route("/tasks/{id}/reviews", get(list).post(start))
}

async fn get_policy(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let p = review::policy(&state.db, project_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "policy": p })))
}

pub(crate) async fn put_policy(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<Policy>,
) -> Result<Json<Value>, ApiError> {
    require_write(&headers, "this decides what a merge requires")?;
    let (kind, workspace): (String, Uuid) =
        sqlx::query_as("SELECT kind, workspace_id FROM projects WHERE id = $1")
            .bind(project_id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?
            .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    if kind != "repo" {
        return Err((
            StatusCode::BAD_REQUEST,
            "a review policy is for code repositories; an app's changes land on their own".into(),
        ));
    }
    if !(1..=3).contains(&body.max_rounds) {
        return Err((
            StatusCode::BAD_REQUEST,
            "rounds of review are 1, 2 or 3".into(),
        ));
    }
    if body.require_review && body.reviewer_agent_id.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "an agent review needs a reviewer agent".into(),
        ));
    }
    if let Some(reviewer) = body.reviewer_agent_id {
        vet_reviewer(&state, reviewer, workspace).await?;
    }
    aichip_core::revisions::keep(
        &state.db,
        aichip_core::revisions::EntityKind::ReviewPolicy,
        &project_id.to_string(),
    )
    .await;
    sqlx::query(
        "INSERT INTO project_review_policy
             (project_id, require_checks, require_review, reviewer_agent_id, max_rounds,
              require_pr_green, run_checks_after_every_run, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now())
         ON CONFLICT (project_id) DO UPDATE
            SET require_checks = EXCLUDED.require_checks,
                require_review = EXCLUDED.require_review,
                reviewer_agent_id = EXCLUDED.reviewer_agent_id,
                max_rounds = EXCLUDED.max_rounds,
                require_pr_green = EXCLUDED.require_pr_green,
                run_checks_after_every_run = EXCLUDED.run_checks_after_every_run,
                updated_at = now()",
    )
    .bind(project_id)
    .bind(body.require_checks)
    .bind(body.require_review)
    .bind(body.reviewer_agent_id)
    .bind(body.max_rounds)
    .bind(body.require_pr_green)
    .bind(body.run_checks_after_every_run)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    get_policy(State(state), Path(project_id)).await
}

/// A reviewer that exists in this workspace, can still be given work, and —
/// where its engine is fixed — runs on one that holds a pass to read-only.
/// Refused here, at the click, rather than discovered after the next run.
async fn vet_reviewer(state: &AppState, reviewer: Uuid, workspace: Uuid) -> Result<(), ApiError> {
    let row = sqlx::query("SELECT workspace_id, engine FROM agents WHERE id = $1")
        .bind(reviewer)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::BAD_REQUEST, "no such agent".to_string()))?;
    if row.get::<Option<Uuid>, _>("workspace_id") != Some(workspace) {
        return Err((
            StatusCode::BAD_REQUEST,
            "the reviewer must be an agent of this project's workspace".into(),
        ));
    }
    aichip_core::agents::assert_assignable(&state.db, reviewer)
        .await
        .map_err(run_refused)?;
    if let Some(engine) = row.get::<Option<String>, _>("engine") {
        let enforces = state
            .orchestrator
            .engine(&engine)
            .is_some_and(|e| e.capabilities().enforces_denied_tools);
        if !enforces {
            return Err((
                StatusCode::CONFLICT,
                review::Skip::CannotEnforce(engine).sentence(),
            ));
        }
    }
    Ok(())
}

/// The card's verdicts, newest first, with where the loop stands and what
/// the gate still wants.
async fn list(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let project: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such card".to_string()))?;
    let rows = sqlx::query(
        "SELECT d.id, d.run_id, d.round, d.verdict, d.submitted, d.summary, d.notes, d.created_at,
                a.name AS reviewer
           FROM review_decisions d LEFT JOIN agents a ON a.id = d.reviewer_agent_id
          WHERE d.task_id = $1 ORDER BY d.created_at DESC LIMIT 50",
    )
    .bind(task_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let reviews: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "runId": r.get::<Option<Uuid>, _>("run_id"),
                "round": r.get::<i32, _>("round"),
                "verdict": r.get::<String, _>("verdict"),
                "submitted": r.get::<bool, _>("submitted"),
                "summary": r.get::<String, _>("summary"),
                "notes": r.get::<Value, _>("notes"),
                "reviewer": r.get::<Option<String>, _>("reviewer"),
                "createdAt": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        })
        .collect();
    let policy = review::policy(&state.db, project).await.map_err(internal)?;
    let rounds = review::rounds(&state.db, task_id).await.map_err(internal)?;
    let unmet = review::gate(&state.db, task_id).await.map_err(internal)?;
    Ok(Json(json!({
        "reviews": reviews,
        "rounds": rounds,
        "maxRounds": policy.max_rounds,
        "requireReview": policy.require_review,
        "unmet": unmet,
    })))
}

/// A person asks for a review now — after the rounds ran out, or to have a
/// second look at work they changed. One round past the cap: the cap bounds
/// what runs unattended, not what a person asks for.
async fn start(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    match review::start(&state.orchestrator, task_id, Start::Person)
        .await
        .map_err(run_refused)?
    {
        Ok(run_id) => Ok(Json(json!({ "runId": run_id }))),
        Err(skip) => Err((StatusCode::CONFLICT, skip.sentence())),
    }
}

#[cfg(test)]
mod tests {
    /// The policy decides what Merge requires; a second writer would be a
    /// way around that. Same rule, same scan, as `project_checks`.
    #[test]
    fn only_this_file_writes_the_review_policy() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/");
        let writes = [
            "INSERT INTO project_review_policy",
            "UPDATE project_review_policy",
            "DELETE FROM project_review_policy",
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
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("routes/reviews.rs")
                {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                // Shipped code only: a database test sets up the policy it
                // tests directly.
                let shipped = source.split("#[cfg(test)]").next().unwrap_or("");
                let flat = shipped.split_whitespace().collect::<Vec<_>>().join(" ");
                if writes.iter().any(|w| flat.contains(w)) {
                    offenders.push(path.display().to_string());
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "only routes/reviews.rs may write project_review_policy: {offenders:#?}"
        );
    }
}
