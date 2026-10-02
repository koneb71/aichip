//! The inbox: everything waiting on a person, and answering it in place.
//!
//! The list is `eren_core::inbox::list`. Resolving is here, because half
//! the answers are route handlers (starting a card, resuming a run) or the
//! permission broker, which only the server holds — and every one of them is
//! the *same* function the matching button calls, never a second copy.

use super::{answer_refused, internal, require_write, ApiError};
use crate::AppState;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use eren_core::decisions::Effect;
use eren_core::inbox::{self, parse_key};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/inbox", get(list))
        .route("/inbox/resolve", post(resolve))
        .route("/inbox/read", post(read))
        .route("/inbox/snooze", post(snooze))
}

#[derive(Deserialize)]
struct ListQuery {
    workspace_id: Uuid,
    /// Include snoozed items.
    #[serde(default)]
    all: bool,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let items = inbox::list(&state.db, q.workspace_id, q.all)
        .await
        .map_err(internal)?;
    let unread = items.iter().filter(|i| !i.read).count();
    Ok(Json(json!({ "items": items, "unread": unread })))
}

#[derive(Deserialize)]
struct Resolve {
    key: String,
    action: String,
    /// A revise note, a rejection reason, or an answer.
    #[serde(default)]
    text: Option<String>,
    /// Approving a proposal to start a card: the person saw what it could
    /// cost and starts it anyway — the Start button's own acknowledgement.
    #[serde(default)]
    acknowledge_forecast: bool,
}

fn bad(msg: impl Into<String>) -> ApiError {
    (StatusCode::BAD_REQUEST, msg.into())
}

async fn resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Resolve>,
) -> Result<Json<Value>, ApiError> {
    require_write(&headers, "this answers something on your behalf")?;
    let key = parse_key(&body.key).ok_or_else(|| bad("not an inbox key"))?;
    let id = |i: usize| key.uuid(i).ok_or_else(|| bad("not an inbox key"));
    let text = body.text.as_deref().unwrap_or("").trim();
    let orch = &state.orchestrator;

    let outcome: Value = match (key.kind.as_str(), body.action.as_str()) {
        ("plan", "approve") => {
            eren_core::approvals::approve_task_plan(orch, id(0)?)
                .await
                .map_err(answer_refused)?;
            json!("approved")
        }
        ("plan", "revise") => {
            eren_core::approvals::revise_task_plan(orch, id(0)?, text)
                .await
                .map_err(answer_refused)?;
            json!("revising")
        }
        ("team_plan", "approve") => {
            eren_core::approvals::approve_org_plan(orch, id(0)?)
                .await
                .map_err(answer_refused)?;
            json!("approved")
        }
        ("team_plan", "reject") => {
            eren_core::approvals::reject_org_plan(orch, id(0)?, Some(text))
                .await
                .map_err(answer_refused)?;
            json!("rejected")
        }
        ("permission", "allow" | "deny") => {
            // The broker is the one place a prompt is answered; the inbox
            // calls it exactly as the card's Allow and Deny do.
            if !state
                .permissions
                .resolve(&key.ids[0], body.action == "allow")
            {
                return Err((
                    StatusCode::CONFLICT,
                    "that request has already been answered or has gone".into(),
                ));
            }
            json!(body.action)
        }
        ("permission_expired", "resume") => {
            let Json(v) =
                super::tasks::resume_run(State(state.clone()), axum::extract::Path(id(0)?)).await?;
            v
        }
        ("question", "answer") => {
            let run = eren_core::asks::answer(orch, id(0)?, text)
                .await
                .map_err(answer_refused)?;
            json!({ "runId": run })
        }
        ("question", "dismiss") => {
            eren_core::asks::dismiss(&state.db, id(0)?)
                .await
                .map_err(answer_refused)?;
            json!("dismissed")
        }
        ("decision", "approve") => decide(&state, id(0)?, body.acknowledge_forecast).await?,
        ("review", "review_again") => {
            let task: Uuid =
                sqlx::query_scalar("SELECT task_id FROM review_decisions WHERE id = $1")
                    .bind(id(0)?)
                    .fetch_optional(&state.db.pool)
                    .await
                    .map_err(internal)?
                    .ok_or((StatusCode::NOT_FOUND, "no such review".to_string()))?;
            match eren_core::review::start(orch, task, eren_core::review::Start::Person)
                .await
                .map_err(super::run_refused)?
            {
                Ok(run_id) => json!({ "runId": run_id }),
                Err(skip) => return Err((StatusCode::CONFLICT, skip.sentence())),
            }
        }
        ("decision", "deny") => {
            let claimed = eren_core::decisions::claim(&state.db, id(0)?, "denied")
                .await
                .map_err(internal)?;
            if claimed.is_none() {
                return Err((
                    StatusCode::CONFLICT,
                    "that proposal has already been decided".into(),
                ));
            }
            json!("denied")
        }
        ("chat_plan", "approve") => {
            let turn = eren_core::approvals::approve_chat_plan(orch, id(0)?, id(1)?, None)
                .await
                .map_err(answer_refused)?;
            json!({ "runId": turn.run_id })
        }
        ("schema", "apply") => {
            let applied = eren_core::apps::apply_plan(&state.db, id(0)?, id(1)?)
                .await
                .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
            json!({ "applied": applied.len() })
        }
        ("schema", "discard") => {
            eren_core::apps::discard_plan(&state.db, id(0)?, id(1)?)
                .await
                .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
            json!("discarded")
        }
        ("kb_revision", "accept") => {
            let seq: i32 = key
                .ids
                .get(1)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| bad("not an inbox key"))?;
            eren_core::kb::revisions::accept(&state.db, id(0)?, seq)
                .await
                .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
            json!("accepted")
        }
        ("kb_revision", "discard") => {
            let seq: i32 = key
                .ids
                .get(1)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| bad("not an inbox key"))?;
            eren_core::kb::revisions::discard(&state.db, id(0)?, seq, text)
                .await
                .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
            json!("discarded")
        }
        ("permission_expired", "dismiss") => {
            // Nothing of its own to close: the run already failed. Out of the
            // list for good.
            inbox::snooze(
                &state.db,
                &body.key,
                chrono::Utc::now() + chrono::Duration::days(3650),
            )
            .await
            .map_err(internal)?;
            json!("dismissed")
        }
        (kind, action) => {
            return Err(bad(format!(
                "{kind} cannot be answered with {action} here — open it instead"
            )))
        }
    };
    let _ = inbox::mark_read(&state.db, &body.key).await;
    Ok(Json(json!({ "ok": true, "outcome": outcome })))
}

/// Approve a proposal: claim it, run its effect through the function its own
/// button calls, and record what happened — including a failure.
///
/// Claim, effect and record run as one task the request cannot drop: a tab
/// closed mid-approval used to leave the proposal claimed and the effect
/// never run. (A crash in between is `recover_orphans`'s to put back.)
///
/// A refusal the person can answer — a 409: the card is blocked for now, the
/// start wants its cost acknowledged — puts the proposal back as it was, so
/// approving again later works. Only a real failure is recorded as one.
async fn decide(state: &AppState, id: Uuid, acknowledge_forecast: bool) -> Result<Value, ApiError> {
    let state = state.clone();
    tokio::spawn(async move {
        let effect = eren_core::decisions::claim(&state.db, id, "approved")
            .await
            .map_err(internal)?
            .ok_or((
                StatusCode::CONFLICT,
                "that proposal has already been decided".to_string(),
            ))?;
        let result = apply(&state, id, &effect, acknowledge_forecast).await;
        match &result {
            Err((StatusCode::CONFLICT, _)) => eren_core::decisions::reopen(&state.db, id).await,
            _ => {
                eren_core::decisions::settle(&state.db, id, result.clone().map_err(|(_, m)| m))
                    .await
            }
        }
        .map_err(internal)?;
        result.map(|o| json!(o))
    })
    .await
    .map_err(internal)?
}

async fn apply(
    state: &AppState,
    decision: Uuid,
    effect: &Effect,
    acknowledge_forecast: bool,
) -> Result<String, ApiError> {
    use axum::extract::Path;
    let workspace: Uuid = sqlx::query_scalar("SELECT workspace_id FROM decisions WHERE id = $1")
        .bind(decision)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?;
    let agent_id = |name: String| async move {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM agents WHERE workspace_id = $1 AND lower(name) = lower($2) AND status <> 'retired'",
        )
        .bind(workspace)
        .bind(&name)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::CONFLICT, format!("there is no agent called {name} any more")))
    };
    match effect {
        Effect::StartCard { card_id } => {
            // The Start button itself: its vet, its forecast question, its door.
            let body = super::tasks::StartBody {
                acknowledge_forecast,
            };
            let Json(v) =
                super::tasks::start(State(state.clone()), Path(*card_id), Some(Json(body))).await?;
            Ok(format!(
                "started run {}",
                v["runId"].as_str().unwrap_or("?")
            ))
        }
        Effect::MoveCard { card_id, column } => {
            let Json(_) = super::tasks::move_task(
                State(state.clone()),
                Path(*card_id),
                Json(super::tasks::MoveTask::to_column(column)),
            )
            .await?;
            Ok(format!("moved to {column}"))
        }
        Effect::AssignCard { card_id, agent } => {
            let id = agent_id(agent.clone()).await?;
            let Json(_) = super::tasks::move_task(
                State(state.clone()),
                Path(*card_id),
                Json(super::tasks::MoveTask::assign(id)),
            )
            .await?;
            Ok(format!("assigned to {agent}"))
        }
        Effect::AddBlocker {
            card_id,
            blocked_by,
        } => {
            eren_core::landing::add_blocker(&state.db, *card_id, *blocked_by)
                .await
                .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
            Ok("blocker added".into())
        }
        Effect::PauseAgent { agent } => {
            let id = agent_id(agent.clone()).await?;
            state
                .orchestrator
                .pause_agent(id, Some("paused from an approved proposal"), false)
                .await
                .map_err(internal)?;
            Ok(format!("{agent} paused"))
        }
    }
}

#[derive(Deserialize)]
struct KeyBody {
    key: String,
}

async fn read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<KeyBody>,
) -> Result<Json<Value>, ApiError> {
    require_write(&headers, "this marks your inbox")?;
    parse_key(&body.key).ok_or_else(|| bad("not an inbox key"))?;
    inbox::mark_read(&state.db, &body.key)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SnoozeBody {
    key: String,
    /// Hours from now, 1 to 720.
    hours: i64,
}

async fn snooze(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SnoozeBody>,
) -> Result<Json<Value>, ApiError> {
    require_write(&headers, "this marks your inbox")?;
    parse_key(&body.key).ok_or_else(|| bad("not an inbox key"))?;
    if !(1..=720).contains(&body.hours) {
        return Err(bad("hours must be between 1 and 720"));
    }
    let until = chrono::Utc::now() + chrono::Duration::hours(body.hours);
    inbox::snooze(&state.db, &body.key, until)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "ok": true, "until": until })))
}
