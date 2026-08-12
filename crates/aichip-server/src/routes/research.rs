//! Deep research: ask a question about a project, get a cited report.
//!
//! The interesting route is save-to-kb, which is **idempotent** — the second
//! click returns the article the first one filed. `researches.kb_article_id`
//! is the token, and its `ON DELETE SET NULL` FK is what makes the button
//! usable again after someone deletes the article in the knowledge base.

use super::{internal, ApiError};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/research", get(list).post(create))
        .route("/research/{id}", get(one).delete(remove))
        .route("/research/{id}/rerun", post(rerun))
        .route("/research/{id}/save-to-kb", post(save_to_kb))
        .route("/research/{id}/cancel", post(cancel))
}

#[derive(Deserialize)]
struct ListFilter {
    /// A project's researches, or — with `workspace_id` instead — the
    /// workspace's *general* ones, which belong to no project.
    project_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
}

/// The latest run per research rides along, because the list view has to say
/// "still running" vs "failed" vs "report ready" without N follow-up calls.
const LATEST_RUN: &str = "LEFT JOIN LATERAL (
    SELECT id, status, error_reason, model, cost_usd FROM runs
     WHERE research_id = rs.id ORDER BY created_at DESC LIMIT 1
) r ON TRUE";

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<ListFilter>,
) -> Result<Json<Value>, ApiError> {
    let (where_clause, key) = match (filter.project_id, filter.workspace_id) {
        (Some(p), _) => ("rs.project_id = $1", p),
        (None, Some(w)) => ("rs.workspace_id = $1 AND rs.project_id IS NULL", w),
        (None, None) => {
            return Err((StatusCode::BAD_REQUEST, "name a project or a workspace".into()))
        }
    };
    let rows = sqlx::query(&format!(
        "SELECT rs.id, rs.question, rs.title, rs.kb_article_id, rs.created_at,
                rs.report_md IS NOT NULL AS has_report,
                r.id AS run_id, r.status AS run_status
         FROM researches rs {LATEST_RUN}
         WHERE {where_clause}
         ORDER BY rs.created_at DESC"
    ))
    .bind(key)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let researches: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "question": r.get::<String, _>("question"),
                "title": r.get::<String, _>("title"),
                "hasReport": r.get::<bool, _>("has_report"),
                "kbArticleId": r.get::<Option<Uuid>, _>("kb_article_id"),
                "runId": r.get::<Option<Uuid>, _>("run_id"),
                "runStatus": r.get::<Option<String>, _>("run_status"),
                "createdAt": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })
        })
        .collect();
    Ok(Json(json!({ "researches": researches })))
}

#[derive(Deserialize)]
struct Create {
    /// Exactly one of these: a project research reads the repo and the web,
    /// a workspace ("general") research reads the web alone.
    project_id: Option<Uuid>,
    workspace_id: Option<Uuid>,
    question: String,
    engine: Option<String>,
    /// Typed so an invented tier is a 422, not a row the orchestrator shrugs
    /// at. Absent means the research defaults: Complex, operator's effort.
    model_tier: Option<aichip_shared::ModelTier>,
    effort: Option<aichip_shared::ReasoningEffort>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<Create>,
) -> Result<Json<Value>, ApiError> {
    if body.question.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "say what to research".into()));
    }
    let (id, run_id) = state
        .orchestrator
        .enqueue_research(
            body.project_id,
            body.workspace_id.filter(|_| body.project_id.is_none()),
            body.question.trim(),
            body.engine.as_deref(),
            body.model_tier,
            body.effort,
        )
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "id": id, "runId": run_id })))
}

/// Everything the detail view needs in one response: the question, the
/// report if there is one, and where the latest run stands — so the page can
/// decide live-view vs report-view without a second request.
async fn one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(&format!(
        "SELECT rs.id, rs.project_id, rs.question, rs.title, rs.report_md,
                rs.kb_article_id, rs.created_at, rs.updated_at,
                rs.model_tier, rs.effort,
                r.id AS run_id, r.status AS run_status, r.error_reason AS run_error,
                r.model AS run_model, r.cost_usd AS run_cost
         FROM researches rs {LATEST_RUN}
         WHERE rs.id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such research".to_string()))?;

    Ok(Json(json!({
        "id": row.get::<Uuid, _>("id"),
        "projectId": row.get::<Option<Uuid>, _>("project_id"),
        "question": row.get::<String, _>("question"),
        "title": row.get::<String, _>("title"),
        "reportMd": row.get::<Option<String>, _>("report_md"),
        "kbArticleId": row.get::<Option<Uuid>, _>("kb_article_id"),
        "runId": row.get::<Option<Uuid>, _>("run_id"),
        "runStatus": row.get::<Option<String>, _>("run_status"),
        "runError": row.get::<Option<String>, _>("run_error"),
        "modelTier": row.get::<Option<String>, _>("model_tier"),
        "effort": row.get::<Option<String>, _>("effort"),
        // What the latest run actually ran as, and what it cost — the report
        // header's stats, not settings.
        "runModel": row.get::<Option<String>, _>("run_model"),
        "runCostUsd": row.get::<Option<f64>, _>("run_cost"),
        "createdAt": row.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        "updatedAt": row.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
    })))
}

#[derive(Deserialize, Default)]
struct Rerun {
    engine: Option<String>,
}

/// A fresh run against the same question. The report is replaced wholesale on
/// completion — a research has one current answer, not a history of them.
async fn rerun(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Option<Json<Rerun>>,
) -> Result<Json<Value>, ApiError> {
    let live: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE research_id = $1
                         AND status NOT IN ('completed','failed','canceled'))",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    if live {
        return Err((
            StatusCode::CONFLICT,
            "this research is already running — cancel it before re-running".into(),
        ));
    }
    // No engine named: reuse whatever the last run used, so re-run means
    // "again", not "again on the machine default".
    let engine = match body.and_then(|Json(b)| b.engine) {
        Some(e) => e,
        None => sqlx::query_scalar::<_, String>(
            "SELECT engine FROM runs WHERE research_id=$1 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .unwrap_or_default(),
    };
    let run_id = state
        .orchestrator
        .enqueue_research_run(id, &engine)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "runId": run_id })))
}

/// Cancel this research's live run, if any. Delegates to the same machinery
/// the task drawer's cancel uses.
async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let run_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM runs WHERE research_id = $1
          AND status NOT IN ('completed','failed','canceled')
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    let Some(run_id) = run_id else {
        return Ok(Json(json!({ "canceled": false, "detail": "nothing is running" })));
    };
    super::tasks::cancel_run(State(state), Path(run_id)).await
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    // A live run first: its engine process would otherwise keep streaming
    // into runs rows the CASCADE is about to delete.
    let live: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM runs WHERE research_id = $1
          AND status NOT IN ('completed','failed','canceled') LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    if let Some(run_id) = live {
        let _ = super::tasks::cancel_run(State(state.clone()), Path(run_id)).await;
    }
    let deleted = sqlx::query("DELETE FROM researches WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?
        .rows_affected();
    if deleted == 0 {
        return Err((StatusCode::NOT_FOUND, "no such research".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

/// File the report into the knowledge base as a draft article.
///
/// Idempotent by design: `kb_article_id` records the first save, and the FK's
/// SET NULL on article deletion is what re-arms the button. The body goes
/// through the same two halves `kb::write_body` is made of — `render::prepare`
/// then `revisions::save_edit` — never a bare UPDATE.
async fn save_to_kb(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT rs.question, rs.title, rs.report_md, rs.kb_article_id, rs.project_id,
                COALESCE(p.workspace_id, rs.workspace_id) AS workspace_id
         FROM researches rs LEFT JOIN projects p ON p.id = rs.project_id
         WHERE rs.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such research".to_string()))?;

    // The second click. The FK nulls this column if the article is deleted,
    // so a non-null id here is an article that still exists.
    if let Some(article_id) = row.get::<Option<Uuid>, _>("kb_article_id") {
        return Ok(Json(json!({ "articleId": article_id, "created": false })));
    }

    let report: String = row
        .get::<Option<String>, _>("report_md")
        .filter(|r| !r.trim().is_empty())
        .ok_or((StatusCode::CONFLICT, "this research has no report yet".to_string()))?;

    let title: String = {
        let t: String = row.get("title");
        if t.trim().is_empty() { row.get::<String, _>("question") } else { t }
    };
    let title: String = title.chars().take(120).collect();

    // The report was fence-scrubbed at write time; conversion and
    // sanitisation happen here, on the way into the article store.
    let html = aichip_core::runs::research::to_html(&report);
    let prepared = aichip_core::kb::render::prepare(&html);

    let run_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM runs WHERE research_id=$1 AND status='completed'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;

    let article_id: Uuid = sqlx::query_scalar(
        "INSERT INTO kb_articles (workspace_id, project_id, title, status, origin, position)
         VALUES ($1, $2, $3, 'draft', 'agent',
                 COALESCE((SELECT max(position) + 1000 FROM kb_articles
                            WHERE workspace_id = $1 AND parent_id IS NULL), 1000))
         RETURNING id",
    )
    .bind(row.get::<Uuid, _>("workspace_id"))
    .bind(row.get::<Option<Uuid>, _>("project_id"))
    .bind(&title)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    aichip_core::kb::revisions::save_edit(
        &state.db,
        article_id,
        aichip_core::kb::revisions::NewRevision {
            title: &title,
            html: &prepared.html,
            text: &prepared.text,
            author: aichip_core::kb::revisions::Author::Agent,
            kind: "agent",
            base_seq: None,
            run_id,
            note: "filed from a deep-research report",
        },
        None,
    )
    .await
    .map_err(internal)?;

    sqlx::query("UPDATE researches SET kb_article_id=$1, updated_at=now() WHERE id=$2")
        .bind(article_id)
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;

    Ok(Json(json!({ "articleId": article_id, "created": true })))
}
