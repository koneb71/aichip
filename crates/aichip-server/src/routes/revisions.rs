//! Configuration history, and putting an old version back.
//!
//! A restore is an edit: the snapshot is mapped onto the entity's own update
//! body and that entity's own handler is called. So it is validated like any
//! edit, it passes the same gates (the write header, the single writer of
//! `project_checks`), it lands in the audit log like any edit, and it keeps a
//! revision of what it replaced — undoing an undo is one more click.

use super::{internal, require_write, ApiError};
use crate::AppState;
use aichip_core::revisions::{self, EntityKind};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
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
    Query(q): Query<ListQ>,
) -> Result<Json<Value>, ApiError> {
    let kind = kind_of(&q.kind)?;
    let revs = revisions::list(&state.db, kind, &q.id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "revisions": revs })))
}

fn uuid(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| (StatusCode::BAD_REQUEST, "not an id".to_string()))
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
    Path(rev): Path<i64>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_write(&headers, "this restores a saved setting")?;
    let (kind, id, snap) = revisions::get(&state.db, rev)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such revision".to_string()))?;
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
            });
            let Json(_) =
                super::manager::upsert(s, Path(uuid(project)?), Json(body(mapped)?)).await?;
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
            let Json(_) = super::routines::update(s, Path(uuid(&id)?), Json(body(mapped)?)).await?;
        }
        EntityKind::Skill => {
            let Json(_) = super::skills::update(s, Path(uuid(&id)?), Json(body(snap)?)).await?;
        }
        EntityKind::ProjectChecks => {
            // Through `routes/checks.rs`, the one writer of check commands.
            let Json(_) =
                super::checks::put_config(s, Path(uuid(&id)?), headers, Json(body(snap)?)).await?;
        }
        EntityKind::BudgetPolicy => {
            let Json(_) =
                super::budgets::update(s, Path(uuid(&id)?), headers, Json(body(snap)?)).await?;
        }
        EntityKind::Attention => {
            let Json(_) = super::settings::set_attention(s, headers, Json(body(snap)?)).await?;
        }
        EntityKind::ReviewPolicy => {
            return Err((
                StatusCode::NOT_IMPLEMENTED,
                "review policies are restored from their own screen".into(),
            ));
        }
    }
    Ok(Json(
        json!({ "restored": true, "kind": kind.as_str(), "id": id }),
    ))
}
