//! Routines: a prompt that runs on a schedule.
//!
//! A routine never executes anything itself — `fire` only *enqueues* through
//! the same doors a person uses (a chat turn, a research, a board card), so
//! concurrency limits, backoff, permission prompts and spend accounting all
//! behave exactly as they do for manual work. What a firing produced is a
//! `routine_runs` row pointing at the ordinary artifact.

use aichip_shared::{ReasoningEffort, TierChoice};
use chrono::Utc;
use sqlx::Row;
use uuid::Uuid;

use crate::db::Db;
use crate::runs::mentions;
use crate::runs::orchestrator::Orchestrator;

/// What one firing produced — the ids the history row records.
#[derive(Debug, Default)]
pub struct Fired {
    pub run_id: Option<Uuid>,
    pub research_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub chat_id: Option<Uuid>,
}

/// Fire one routine and record the firing, success or not.
///
/// A failed enqueue is history too — the row carries the error instead of an
/// artifact, so the routine's list shows "didn't run: the assistant was still
/// working" rather than silently thinning out.
pub async fn fire(
    db: &Db,
    orchestrator: &Orchestrator,
    routine_id: Uuid,
    trigger: &str,
) -> anyhow::Result<()> {
    let outcome = dispatch(db, orchestrator, routine_id).await;
    let (fired, error) = match &outcome {
        Ok(f) => (Some(f), None),
        Err(e) => (None, Some(e.to_string())),
    };
    sqlx::query(
        "INSERT INTO routine_runs (routine_id, trigger, run_id, research_id, task_id, chat_id, error)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(routine_id)
    .bind(trigger)
    .bind(fired.and_then(|f| f.run_id))
    .bind(fired.and_then(|f| f.research_id))
    .bind(fired.and_then(|f| f.task_id))
    .bind(fired.and_then(|f| f.chat_id))
    .bind(error)
    .execute(&db.pool)
    .await?;
    outcome.map(|_| ())
}

async fn dispatch(db: &Db, orchestrator: &Orchestrator, routine_id: Uuid) -> anyhow::Result<Fired> {
    let r = sqlx::query(
        "SELECT workspace_id, name, kind, project_id, prompt, engine, model_tier, effort, chat_id
         FROM routines WHERE id = $1",
    )
    .bind(routine_id)
    .fetch_optional(&db.pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("no such routine"))?;

    let kind: String = r.get("kind");
    let workspace_id: Uuid = r.get("workspace_id");
    let project_id: Option<Uuid> = r.get("project_id");
    let prompt: String = r.get("prompt");
    let engine = r
        .get::<Option<String>, _>("engine")
        .unwrap_or_else(|| orchestrator.default_engine());
    // Fail here, in one place, rather than three different ways downstream.
    if orchestrator.engine(&engine).is_none() {
        anyhow::bail!("{engine} isn't installed on this machine");
    }
    // Stored as choice text ("auto" included) — the same column format the
    // chats and tasks tables use, so it binds straight through.
    let tier: Option<TierChoice> = r
        .get::<Option<String>, _>("model_tier")
        .as_deref()
        .and_then(TierChoice::parse);
    let effort: Option<ReasoningEffort> = r
        .get::<Option<String>, _>("effort")
        .as_deref()
        .and_then(ReasoningEffort::parse);

    match kind.as_str() {
        "chat" => {
            fire_chat(
                db,
                orchestrator,
                routine_id,
                workspace_id,
                project_id,
                r.get("chat_id"),
                &r.get::<String, _>("name"),
                &prompt,
                &engine,
                tier,
                effort,
            )
            .await
        }
        "research" => {
            let (research_id, run_id) = orchestrator
                .enqueue_research(
                    project_id,
                    // Exactly one owner, same rule the table CHECKs.
                    if project_id.is_none() { Some(workspace_id) } else { None },
                    &prompt,
                    Some(&engine),
                    // Research has no auto-routing; "auto" falls to the
                    // research default (Complex) by passing None.
                    tier.and_then(TierChoice::fixed),
                    effort,
                )
                .await?;
            Ok(Fired { research_id: Some(research_id), run_id: Some(run_id), ..Default::default() })
        }
        "task" => fire_task(db, orchestrator, &r.get::<String, _>("name"), project_id, &prompt, &engine, tier, effort).await,
        other => anyhow::bail!("unknown routine kind {other:?}"),
    }
}

/// Append the prompt as a user turn in the routine's standing thread.
///
/// One thread rather than a chat per firing: the value of a recurring
/// question is the running record of its answers, and session resume means
/// the assistant remembers what it said yesterday.
#[allow(clippy::too_many_arguments)]
async fn fire_chat(
    db: &Db,
    orchestrator: &Orchestrator,
    routine_id: Uuid,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    chat_id: Option<Uuid>,
    name: &str,
    prompt: &str,
    engine: &str,
    tier: Option<TierChoice>,
    effort: Option<ReasoningEffort>,
) -> anyhow::Result<Fired> {
    // The thread is created on first fire (and re-created if deleted — the
    // FK nulled our link). Titled after the routine so it is findable.
    let chat_id = match chat_id {
        Some(id) => id,
        None => {
            let id: Uuid = sqlx::query_scalar(
                "INSERT INTO chats (project_id, workspace_id, title) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(project_id)
            .bind(if project_id.is_none() { Some(workspace_id) } else { None })
            .bind(name)
            .fetch_one(&db.pool)
            .await?;
            sqlx::query("UPDATE routines SET chat_id = $1 WHERE id = $2")
                .bind(id)
                .bind(routine_id)
                .execute(&db.pool)
                .await?;
            id
        }
    };

    // The same guard as the send route's 409: two concurrent turns race on
    // the chat's session id. Skipping a firing is recorded as its error.
    let busy: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE chat_id = $1
            AND status IN ('queued','starting','running','waiting_permission','rate_limited'))",
    )
    .bind(chat_id)
    .fetch_one(&db.pool)
    .await?;
    if busy {
        anyhow::bail!("the assistant was still working on the previous turn");
    }

    if tier.is_some() || effort.is_some() {
        sqlx::query(
            "UPDATE chats SET model_tier = coalesce($2, model_tier),
                              effort = coalesce($3, effort) WHERE id = $1",
        )
        .bind(chat_id)
        .bind(tier.map(|t| t.as_str().to_string()))
        .bind(effort.map(|e| e.as_str().to_string()))
        .execute(&db.pool)
        .await?;
    }

    let message_id: Uuid = sqlx::query_scalar(
        "INSERT INTO chat_messages (chat_id, role, content) VALUES ($1, 'user', $2) RETURNING id",
    )
    .bind(chat_id)
    .bind(prompt)
    .fetch_one(&db.pool)
    .await?;

    // A routine prompt may name @agents and @skills like any typed turn.
    let (agents, skills) = mentions::resolve_all(db, workspace_id, prompt).await?;
    mentions::record(db, message_id, &agents).await?;
    mentions::record_skills(db, message_id, &skills).await?;

    // Float the thread so this morning's answer is at the top of the list.
    sqlx::query("UPDATE chats SET updated_at = now() WHERE id = $1")
        .bind(chat_id)
        .execute(&db.pool)
        .await?;

    let run_id = orchestrator.enqueue_chat_turn(chat_id, engine).await?;
    Ok(Fired { chat_id: Some(chat_id), run_id: Some(run_id), ..Default::default() })
}

/// Create a card on the project's board and start it.
async fn fire_task(
    db: &Db,
    orchestrator: &Orchestrator,
    name: &str,
    project_id: Option<Uuid>,
    prompt: &str,
    engine: &str,
    tier: Option<TierChoice>,
    effort: Option<ReasoningEffort>,
) -> anyhow::Result<Fired> {
    let Some(project_id) = project_id else {
        anyhow::bail!("a task routine needs a project board");
    };
    // Dated so the board reads as a series, not a pile of identical cards.
    let title = format!("{name} · {}", Utc::now().format("%b %-d"));
    // permission_mode stays NULL — the card inherits the project's default
    // *when it runs*, exactly like a hand-made card. A "reviewed" default
    // parks the run at its first permission prompt, which is the attention
    // system's job to surface, not a reason to escalate an unattended run.
    let task_id: Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (project_id, title, prompt, model_tier, engine, board_column, effort)
         VALUES ($1, $2, $3, $4, $5, 'backlog', $6) RETURNING id",
    )
    .bind(project_id)
    .bind(&title)
    .bind(prompt)
    .bind(tier.unwrap_or(TierChoice::Medium).as_str())
    .bind(engine)
    .bind(effort.map(|e| e.as_str().to_string()))
    .fetch_one(&db.pool)
    .await?;

    match orchestrator.enqueue_task(task_id).await {
        Ok(run_id) => {
            sqlx::query("UPDATE tasks SET board_column='running' WHERE id=$1")
                .bind(task_id)
                .execute(&db.pool)
                .await?;
            Ok(Fired { task_id: Some(task_id), run_id: Some(run_id), ..Default::default() })
        }
        // The card exists but did not start — leave it in the backlog where
        // it is visible and startable by hand, and say so in the history.
        Err(e) => Err(anyhow::anyhow!("the card was created but did not start: {e}")),
    }
}
