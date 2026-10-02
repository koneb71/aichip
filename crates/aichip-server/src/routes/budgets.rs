//! Budget policies: what each scope may spend, where it stands, and the
//! override that lets held work go. The rules live in `aichip_core::budgets`;
//! this is the screen's view of them.
//!
//! Writes need the `x-aichip-write` header, for the reason `checks.rs` and the
//! attention hook give: there is no CORS layer, so a browser cannot send it
//! cross-origin, and a page elsewhere must not be able to lift a cap that is
//! there to stop spending.

use super::{internal, ApiError};
use crate::AppState;
use aichip_core::budgets::{self, Policy, ScopeKind, WindowKind};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

const WRITE_HEADER: &str = "x-aichip-write";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/budgets", get(list).post(create))
        .route("/budgets/{id}", patch(update).delete(remove))
        .route("/budgets/{id}/override", post(override_budget))
        .route("/budgets/{id}/incidents", get(incidents))
        .route("/tasks/{id}/estimate", get(task_estimate))
        .route("/estimate", get(estimate))
}

/// What starting this card is likely to cost, from similar runs.
async fn task_estimate(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let estimate = aichip_core::estimate::for_card(&state.db, task_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "estimate": estimate })))
}

#[derive(Deserialize)]
struct EstimateQuery {
    project_id: Uuid,
    tier: Option<String>,
    engine: Option<String>,
}

/// The same for a card not made yet — the new-card form asks before it
/// exists.
async fn estimate(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<EstimateQuery>,
) -> Result<Json<Value>, ApiError> {
    let engine = q
        .engine
        .unwrap_or_else(|| state.orchestrator.default_engine());
    let tier = q.tier.filter(|t| t != "auto");
    let estimate =
        aichip_core::estimate::for_task(&state.db, q.project_id, tier.as_deref(), &engine)
            .await
            .map_err(internal)?;
    Ok(Json(json!({ "estimate": estimate })))
}

fn require_write_header(headers: &HeaderMap) -> Result<(), ApiError> {
    if headers.contains_key(WRITE_HEADER) {
        Ok(())
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            format!("changing a budget needs the {WRITE_HEADER} header"),
        ))
    }
}

/// Every policy with where it stands this window, and the engines a dollar
/// cap cannot see.
async fn list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut out = vec![];
    for policy in budgets::list(&state.db).await.map_err(internal)? {
        let scope = scope_label(&state, policy.scope_kind, policy.scope_id).await?;
        let standing = budgets::standing(&state.db, policy)
            .await
            .map_err(internal)?;
        let held: i64 = sqlx::query_scalar("SELECT count(*) FROM queue WHERE held_by = $1")
            .bind(standing.policy.id)
            .fetch_one(&state.db.pool)
            .await
            .map_err(internal)?;
        let mut row = serde_json::to_value(&standing).map_err(internal)?;
        row["scopeLabel"] = json!(scope);
        row["held"] = json!(held);
        row["forecast"] =
            serde_json::to_value(aichip_core::estimate::burn(&standing)).map_err(internal)?;
        out.push(row);
    }
    // Named, not inferred from an engine id: whichever engines say they never
    // report a price are the ones a dollar-only cap is blind to.
    let unpriced: Vec<&'static str> = state
        .orchestrator
        .engines()
        .iter()
        .filter(|e| !e.capabilities().reports_cost)
        .map(|e| e.label())
        .collect();
    Ok(Json(
        json!({ "policies": out, "unpricedEngines": unpriced }),
    ))
}

/// The thing a policy covers, in words: "Project · checkout-api".
async fn scope_label(
    state: &AppState,
    kind: ScopeKind,
    id: Option<Uuid>,
) -> Result<String, ApiError> {
    // Each table name is a literal chosen by the enum, never request text.
    let table = match kind {
        ScopeKind::Machine => return Ok("This machine".into()),
        ScopeKind::Workspace => "workspaces",
        ScopeKind::Project => "projects",
        ScopeKind::Agent => "agents",
        ScopeKind::Team => "teams",
        ScopeKind::Routine => "routines",
    };
    let name: Option<String> =
        sqlx::query_scalar(&format!("SELECT name FROM {table} WHERE id = $1"))
            .bind(id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?;
    let noun = match kind {
        ScopeKind::Workspace => "Workspace",
        ScopeKind::Project => "Project",
        ScopeKind::Agent => "Agent",
        ScopeKind::Team => "Team",
        _ => "Routine",
    };
    Ok(format!(
        "{noun} · {}",
        name.unwrap_or_else(|| "(deleted)".into())
    ))
}

#[derive(Deserialize)]
struct Body {
    name: String,
    scope_kind: String,
    scope_id: Option<Uuid>,
    #[serde(default = "day")]
    window_kind: String,
    cap_usd: Option<f64>,
    cap_output_tokens: Option<i64>,
    cap_runs: Option<i32>,
    #[serde(default = "eighty")]
    warn_percent: i32,
    #[serde(default = "hold")]
    on_exceed: String,
    confirm_above_usd: Option<f64>,
    #[serde(default = "yes")]
    enabled: bool,
}

fn day() -> String {
    "day".into()
}
fn eighty() -> i32 {
    80
}
fn hold() -> String {
    "hold".into()
}
fn yes() -> bool {
    true
}

/// A body checked field by field, so a refusal names what to fix rather
/// than surfacing a constraint name from the database.
struct Valid {
    name: String,
    scope_kind: ScopeKind,
    scope_id: Option<Uuid>,
    window_kind: WindowKind,
    body: Body,
}

async fn validate(state: &AppState, body: Body) -> Result<Valid, ApiError> {
    let bad = |m: &str| (StatusCode::BAD_REQUEST, m.to_string());
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(bad("name: a budget needs a name"));
    }
    let scope_kind = ScopeKind::parse(&body.scope_kind)
        .ok_or_else(|| bad("scope_kind: machine, workspace, project, agent, team or routine"))?;
    let window_kind = WindowKind::parse(&body.window_kind)
        .ok_or_else(|| bad("window_kind: day, week or month"))?;
    match (scope_kind, body.scope_id) {
        (ScopeKind::Machine, Some(_)) => {
            return Err(bad("scope_id: a machine budget covers no one thing"))
        }
        (ScopeKind::Machine, None) => {}
        (_, None) => return Err(bad("scope_id: say which one this budget covers")),
        (kind, Some(_)) => {
            if scope_label(state, kind, body.scope_id)
                .await?
                .ends_with("(deleted)")
            {
                return Err(bad("scope_id: there is no such thing to budget"));
            }
        }
    }
    if body.cap_usd.is_none() && body.cap_output_tokens.is_none() && body.cap_runs.is_none() {
        return Err(bad(
            "caps: set at least one of dollars, output tokens or runs",
        ));
    }
    if body.cap_usd.is_some_and(|c| c <= 0.0)
        || body.cap_output_tokens.is_some_and(|c| c <= 0)
        || body.cap_runs.is_some_and(|c| c <= 0)
    {
        return Err(bad(
            "caps: a cap must be more than zero — to stop everything, pause the queue",
        ));
    }
    if !(1..=100).contains(&body.warn_percent) {
        return Err(bad("warn_percent: between 1 and 100"));
    }
    if body.on_exceed != "hold" && body.on_exceed != "stop" {
        return Err(bad("on_exceed: hold or stop"));
    }
    if body.confirm_above_usd.is_some_and(|c| c < 0.0) {
        return Err(bad("confirm_above_usd: cannot be negative"));
    }
    Ok(Valid {
        name,
        scope_kind,
        scope_id: body.scope_id,
        window_kind,
        body,
    })
}

async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Body>,
) -> Result<Json<Value>, ApiError> {
    require_write_header(&headers)?;
    let v = validate(&state, body).await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO budget_policies
            (name, scope_kind, scope_id, window_kind, cap_usd, cap_output_tokens, cap_runs,
             warn_percent, on_exceed, confirm_above_usd, enabled)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING id",
    )
    .bind(&v.name)
    .bind(v.scope_kind.as_str())
    .bind(v.scope_id)
    .bind(v.window_kind.as_str())
    .bind(v.body.cap_usd)
    .bind(v.body.cap_output_tokens)
    .bind(v.body.cap_runs)
    .bind(v.body.warn_percent)
    .bind(&v.body.on_exceed)
    .bind(v.body.confirm_above_usd)
    .bind(v.body.enabled)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "id": id })))
}

/// The whole policy, replaced — a budget is small enough that partial edits
/// would only add ways to leave it half-changed.
async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<Body>,
) -> Result<Json<Value>, ApiError> {
    require_write_header(&headers)?;
    let v = validate(&state, body).await?;
    let changed = sqlx::query(
        "UPDATE budget_policies SET
            name = $2, scope_kind = $3, scope_id = $4, window_kind = $5, cap_usd = $6,
            cap_output_tokens = $7, cap_runs = $8, warn_percent = $9, on_exceed = $10,
            confirm_above_usd = $11, enabled = $12
          WHERE id = $1",
    )
    .bind(id)
    .bind(&v.name)
    .bind(v.scope_kind.as_str())
    .bind(v.scope_id)
    .bind(v.window_kind.as_str())
    .bind(v.body.cap_usd)
    .bind(v.body.cap_output_tokens)
    .bind(v.body.cap_runs)
    .bind(v.body.warn_percent)
    .bind(&v.body.on_exceed)
    .bind(v.body.confirm_above_usd)
    .bind(v.body.enabled)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?
    .rows_affected();
    if changed == 0 {
        return Err((StatusCode::NOT_FOUND, "no such budget".into()));
    }
    // The answer for what it was holding may be different now.
    budgets::release_held(&state.db, id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "updated": true })))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require_write_header(&headers)?;
    budgets::release_held(&state.db, id)
        .await
        .map_err(internal)?;
    sqlx::query("DELETE FROM budget_policies WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
struct Override {
    usd: Option<f64>,
    tokens: Option<i64>,
    runs: Option<i32>,
    /// Why — kept with the incident, so the history says who decided what.
    #[serde(default)]
    note: String,
}

/// More room for this window only, recorded with its reason. The policy is
/// unchanged: next window it is the same budget again.
async fn override_budget(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<Override>,
) -> Result<Json<Value>, ApiError> {
    require_write_header(&headers)?;
    let positive = body.usd.is_some_and(|v| v > 0.0)
        || body.tokens.is_some_and(|v| v > 0)
        || body.runs.is_some_and(|v| v > 0);
    if !positive {
        return Err((
            StatusCode::BAD_REQUEST,
            "say how much more: dollars, output tokens or runs".into(),
        ));
    }
    let policy: Policy = budgets::get(&state.db, id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such budget".to_string()))?;
    let (window_start, _) = budgets::window(&state.db, policy.window_kind)
        .await
        .map_err(internal)?;
    sqlx::query(
        "INSERT INTO budget_incidents (policy_id, window_start, kind, usd, tokens, runs, note)
         VALUES ($1, $2, 'override', $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(window_start)
    .bind(body.usd.filter(|v| *v > 0.0))
    .bind(body.tokens.filter(|v| *v > 0))
    .bind(body.runs.filter(|v| *v > 0))
    .bind(body.note.trim())
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    let released = budgets::release_held(&state.db, id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "overridden": true, "released": released })))
}

async fn incidents(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows: Vec<(
        String,
        chrono::DateTime<chrono::Utc>,
        Option<f64>,
        Option<i64>,
        Option<i32>,
        Option<String>,
        chrono::DateTime<chrono::Utc>,
    )> = sqlx::query_as(
        "SELECT kind, window_start, usd, tokens, runs, note, created_at
               FROM budget_incidents WHERE policy_id = $1
              ORDER BY created_at DESC LIMIT 50",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "incidents": rows.into_iter().map(|(kind, window_start, usd, tokens, runs, note, at)| json!({
            "kind": kind, "windowStart": window_start, "usd": usd, "tokens": tokens,
            "runs": runs, "note": note, "at": at,
        })).collect::<Vec<_>>()
    })))
}
