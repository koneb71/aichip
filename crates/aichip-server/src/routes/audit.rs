//! Reading the ledger: a filterable list, a CSV export, and one card's whole
//! story — comments, runs, checks and what was done to it — in one timeline.

use super::{internal, ApiError};
use crate::AppState;
use aichip_core::audit::{self, Filter};
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/audit", get(list))
        .route("/audit.csv", get(export))
        .route("/tasks/{id}/timeline", get(timeline))
}

#[derive(Deserialize)]
struct Q {
    entity_kind: Option<String>,
    entity_id: Option<String>,
    actor_kind: Option<String>,
    before: Option<i64>,
    limit: Option<i64>,
}

impl Q {
    fn filter(self, default_limit: i64) -> Filter {
        Filter {
            entity_kind: self.entity_kind.filter(|s| !s.is_empty()),
            entity_id: self.entity_id.filter(|s| !s.is_empty()),
            actor_kind: self.actor_kind.filter(|s| !s.is_empty()),
            before: self.before,
            limit: self.limit.unwrap_or(default_limit),
        }
    }
}

async fn list(State(state): State<AppState>, Query(q): Query<Q>) -> Result<Json<Value>, ApiError> {
    let rows = audit::list(&state.db, &q.filter(100))
        .await
        .map_err(internal)?;
    let next = (rows.len() as i64 >= 1)
        .then(|| rows.last().map(|r| r.id))
        .flatten();
    Ok(Json(json!({ "entries": rows, "next": next })))
}

async fn export(
    State(state): State<AppState>,
    Query(q): Query<Q>,
) -> Result<impl IntoResponse, ApiError> {
    let mut f = q.filter(1000);
    f.limit = f.limit.min(1000);
    let rows = audit::list(&state.db, &f).await.map_err(internal)?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"aichip-audit.csv\"",
            ),
        ],
        audit::csv(&rows),
    ))
}

/// Everything that happened on one card, oldest first: what people and
/// agents said, every run and how it ended, every check, and every recorded
/// action on it. One list, so "what happened here overnight" is one read.
async fn timeline(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let mut events: Vec<(chrono::DateTime<chrono::Utc>, Value)> = vec![];

    for r in sqlx::query(
        "SELECT c.created_at, c.author, c.content, a.name AS agent
           FROM task_comments c LEFT JOIN agents a ON a.id = c.agent_id
          WHERE c.task_id = $1",
    )
    .bind(task_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?
    {
        let author: String = r.get("author");
        let who = r
            .get::<Option<String>, _>("agent")
            .unwrap_or_else(|| match author.as_str() {
                "user" => "You".into(),
                "system" => "aichip".into(),
                other => other.into(),
            });
        let content: String = r.get("content");
        events.push((
            r.get("created_at"),
            json!({ "kind": "comment", "actor": who, "title": "commented", "detail": content.chars().take(280).collect::<String>() }),
        ));
    }

    for r in sqlx::query(
        "SELECT r.id, r.created_at, r.finished_at, r.status, r.trigger, r.cost_usd, r.error_reason, a.name AS agent
           FROM runs r LEFT JOIN agents a ON a.id = r.agent_id
          WHERE r.task_id = $1",
    )
    .bind(task_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?
    {
        let trigger: String = r.get("trigger");
        let status: String = r.get("status");
        let cost: Option<f64> = r.get("cost_usd");
        events.push((
            r.get("created_at"),
            json!({
                "kind": "run",
                "runId": r.get::<Uuid, _>("id"),
                "actor": r.get::<Option<String>, _>("agent"),
                "title": format!("{} run {}", trigger, status.replace('_', " ")),
                "detail": r.get::<Option<String>, _>("error_reason"),
                "costUsd": cost,
                "status": status,
            }),
        ));
    }

    for r in sqlx::query("SELECT created_at, status, started_by FROM check_runs WHERE task_id = $1")
        .bind(task_id)
        .fetch_all(&state.db.pool)
        .await
        .map_err(internal)?
    {
        let by: String = r.get("started_by");
        events.push((
            r.get("created_at"),
            json!({ "kind": "checks", "actor": if by == "auto" { "aichip" } else { "You" }, "title": format!("checks {}", r.get::<String, _>("status")) }),
        ));
    }

    let rows = audit::list(
        &state.db,
        &Filter {
            entity_kind: Some("tasks".into()),
            entity_id: Some(task_id.to_string()),
            limit: 200,
            ..Default::default()
        },
    )
    .await
    .map_err(internal)?;
    for r in rows {
        events.push((
            r.at,
            json!({ "kind": "audit", "actor": r.actor_kind, "title": r.summary, "detail": Value::Null }),
        ));
    }

    events.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Json(json!({
        "events": events.into_iter().map(|(at, mut v)| { v["at"] = json!(at); v }).collect::<Vec<_>>()
    })))
}
