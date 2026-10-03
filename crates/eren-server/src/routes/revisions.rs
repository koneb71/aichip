//! Configuration history, and putting an old version back.
//!
//! A restore is an edit: the snapshot is mapped onto the entity's own update
//! body and that entity's own handler is called. So it is validated like any
//! edit, it passes the same gates (the write header, the single writer of
//! `project_checks`), it lands in the audit log like any edit, and it keeps a
//! revision of what it replaced — undoing an undo is one more click.

use super::{internal, require_write, ApiError};
use crate::auth::{Admin, Caller};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use eren_core::revisions::{self, EntityKind};
use eren_core::scope::Owned;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/revisions", get(list))
        .route("/revisions/{rev}/restore", post(restore))
}

#[derive(Deserialize)]
struct ListQ {
    kind: String,
    id: String,
}

fn kind_of(s: &str) -> Result<EntityKind, ApiError> {
    EntityKind::parse(s).ok_or((StatusCode::BAD_REQUEST, format!("{s} has no history")))
}

async fn list(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<ListQ>,
) -> Result<Json<Value>, ApiError> {
    let kind = kind_of(&q.kind)?;
    require_entity(&state, &caller, kind, &q.id, false).await?;
    let revs = revisions::list(&state.db, kind, &q.id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "revisions": revs })))
}

fn uuid(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| (StatusCode::BAD_REQUEST, "not an id".to_string()))
}

/// Refuse unless the entity a history belongs to is the caller's: the
/// machine's settings are the admin's, anything else resolves to the
/// workspace it lives in. Before any revision is read — a snapshot is the
/// whole row, as private as the thing itself.
async fn require_entity(
    state: &AppState,
    caller: &Caller,
    kind: EntityKind,
    id: &str,
    write: bool,
) -> Result<(), ApiError> {
    // Accounts off: everything is the one person's, and an id that is not a
    // uuid answers as it always did (an empty history, a 404 on restore).
    if matches!(caller, Caller::Local) {
        return Ok(());
    }
    let what = match kind {
        EntityKind::Attention | EntityKind::Unattended => {
            return if caller.is_admin() {
                Ok(())
            } else {
                Err((
                    StatusCode::FORBIDDEN,
                    "Only the admin can see or change this; it belongs to the whole machine."
                        .into(),
                ))
            };
        }
        EntityKind::BudgetPolicy => {
            return super::budgets::require_policy(state, caller, uuid(id)?, write).await;
        }
        EntityKind::Agent => Owned::Agent(uuid(id)?),
        EntityKind::Team => Owned::Team(uuid(id)?),
        EntityKind::Routine => Owned::Routine(uuid(id)?),
        EntityKind::Skill => Owned::Skill(uuid(id)?),
        // Both are keyed by their project.
        EntityKind::ProjectChecks | EntityKind::ReviewPolicy => Owned::Project(uuid(id)?),
    };
    caller.require(state, what).await
}

fn body<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, ApiError> {
    serde_json::from_value(v).map_err(|e| {
        (
            StatusCode::CONFLICT,
            format!("that version no longer fits this setting's shape: {e}"),
        )
    })
}

async fn restore(
    State(state): State<AppState>,
    caller: Caller,
    Path(rev): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_write(&headers, "this restores a saved setting")?;
    let (kind, id, snap) = revisions::get(&state.db, rev)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such revision".to_string()))?;
    // Checked here, before the entity is read. A handler that takes a
    // caller is handed this one and checks again, along with whatever the
    // snapshot names (a manager's project, a budget's scope); an agent's and
    // a team's `update` are their routes' caller-free core, so this check is
    // theirs — and a snapshot only names what that row once held.
    require_entity(&state, &caller, kind, &id, true).await?;
    if revisions::current(&state.db, kind, &id)
        .await
        .map_err(internal)?
        .is_none()
    {
        return Err((
            StatusCode::NOT_FOUND,
            "it was deleted, so there is nothing to put this back on".into(),
        ));
    }
    let s = State(state.clone());
    match kind {
        EntityKind::Agent => {
            let Json(_) = super::agents::update(s, Path(uuid(&id)?), Json(body(snap)?)).await?;
        }
        EntityKind::Team => {
            let Json(_) = super::teams::update(s, Path(uuid(&id)?), Json(body(snap)?)).await?;
        }
        EntityKind::Routine if snap["kind"] == "manage" => {
            let project = snap["project_id"].as_str().ok_or((
                StatusCode::CONFLICT,
                "this manager has no project".to_string(),
            ))?;
            let mapped = json!({
                "agentId": snap["agent_id"],
                "brief": snap["prompt"],
                "cronExpr": snap["cron_expr"],
                "catchUp": snap["catch_up"],
                "enabled": snap["enabled"],
                "engine": snap["engine"],
                "modelTier": snap["model_tier"],
                "effort": snap["effort"],
                "maxStarts": snap["max_starts"],
                "onEvents": snap["on_events"],
                "cooldownSecs": snap["cooldown_secs"],
                "maxPassesPerDay": snap["max_passes_per_day"],
                "goalId": snap["goal_id"],
            });
            let Json(_) = super::manager::upsert(
                s,
                caller.clone(),
                Path(uuid(project)?),
                Json(body(mapped)?),
            )
            .await?;
        }
        EntityKind::Routine => {
            let mapped = json!({
                "name": snap["name"],
                "prompt": snap["prompt"],
                "url": snap["url"],
                "cronExpr": snap["cron_expr"],
                "catchUp": snap["catch_up"],
                "enabled": snap["enabled"],
                "engine": snap["engine"],
                "modelTier": snap["model_tier"],
                "effort": snap["effort"],
            });
            let Json(_) =
                super::routines::update(s, caller.clone(), Path(uuid(&id)?), Json(body(mapped)?))
                    .await?;
        }
        EntityKind::Skill => {
            let Json(_) =
                super::skills::update(s, caller.clone(), Path(uuid(&id)?), Json(body(snap)?))
                    .await?;
        }
        EntityKind::ProjectChecks => {
            // Through `routes/checks.rs`, the one writer of check commands.
            let Json(_) = super::checks::put_config(
                s,
                caller.clone(),
                Path(uuid(&id)?),
                headers,
                Json(body(snap)?),
            )
            .await?;
        }
        EntityKind::BudgetPolicy => {
            let Json(_) = super::budgets::update(
                s,
                caller.clone(),
                Path(uuid(&id)?),
                headers,
                Json(body(snap)?),
            )
            .await?;
        }
        EntityKind::Attention => {
            let Json(_) = super::settings::set_attention(
                s,
                Admin(caller.clone()),
                headers,
                Json(body(snap)?),
            )
            .await?;
        }
        EntityKind::Unattended => {
            let Json(_) = super::settings::set_unattended(
                s,
                Admin(caller.clone()),
                headers,
                Json(body(snap)?),
            )
            .await?;
        }
        EntityKind::ReviewPolicy => {
            // Through `routes/reviews.rs`, the one writer of review policies.
            let mapped = json!({
                "requireChecks": snap["require_checks"],
                "requireReview": snap["require_review"],
                "reviewerAgentId": snap["reviewer_agent_id"],
                "maxRounds": snap["max_rounds"],
                "requirePrGreen": snap["require_pr_green"],
                "runChecksAfterEveryRun": snap["run_checks_after_every_run"],
            });
            let Json(_) = super::reviews::put_policy(
                s,
                caller.clone(),
                Path(uuid(&id)?),
                headers,
                Json(body(mapped)?),
            )
            .await?;
        }
    }
    Ok(Json(
        json!({ "restored": true, "kind": kind.as_str(), "id": id }),
    ))
}
