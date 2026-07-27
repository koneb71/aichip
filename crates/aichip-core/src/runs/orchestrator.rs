//! Run orchestrator: owns the run state machine, the concurrency semaphore,
//! and the queue loop. All status transitions happen here and every event is
//! persisted before it is broadcast, so the DB is the source of truth.
//!
//! Two kinds of runs share the same machinery:
//! - task runs (board cards): isolated worktree, permission proxy
//! - chat runs (project assistant turns): real checkout cwd, but locked to a
//!   read-only + aichip-tools allowed list — never Bash/Edit/Write there.

use aichip_shared::{AichipEvent, EventEnvelope, ModelTier, PermissionMode, RunStatus, TierMapping};
use aichip_engines::{Engine, RunSpec};
use chrono::{DateTime, Utc};
use sqlx::Row;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, Semaphore};
use uuid::Uuid;

use crate::bus::EventBus;
use crate::db::Db;
use crate::queue::rate_limit_backoff;
use crate::worktrees::manager::WorktreeManager;

/// Tools a chat run may use: read-only inspection of the checkout plus
/// aichip's own workspace tools. Compliance-adjacent invariant: never add
/// Bash/Edit/Write here — chat runs execute in the user's real checkout.
pub const CHAT_ALLOWED_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "mcp__aichip__create_task",
    "mcp__aichip__start_task",
    "mcp__aichip__list_tasks",
    "mcp__aichip__get_task_status",
    "mcp__aichip__list_agents",
];

const CHAT_SYSTEM_PROMPT: &str = "You are the aichip project assistant embedded in a workspace \
dashboard. You can inspect this repository (Read/Grep/Glob) but you cannot edit it directly. \
To do coding work, create a task with mcp__aichip__create_task (set start=true to launch it \
immediately); each task runs a coding agent in an isolated git worktree and its result appears \
on the user's board for review. Use mcp__aichip__list_tasks / get_task_status to report \
progress, and mcp__aichip__list_agents to pick a specialized agent for a task. Keep replies \
short and conversational; the user sees them in a chat panel.";

pub struct Orchestrator {
    pub db: Db,
    pub bus: EventBus,
    engines: HashMap<&'static str, Arc<dyn Engine>>,
    pub tiers: TierMapping,
    pub worktrees: Arc<WorktreeManager>,
    semaphore: Arc<Semaphore>,
    /// Base URL of aichip's MCP endpoints, e.g. "http://127.0.0.1:4820".
    /// None disables MCP wiring (mock engine / tests).
    mcp_base_url: Option<String>,
    cancels: Mutex<HashMap<Uuid, oneshot::Sender<()>>>,
}

struct StreamOutcome {
    status: RunStatus,
    reason: Option<String>,
}

impl Orchestrator {
    pub fn new(
        db: Db,
        bus: EventBus,
        worktrees: Arc<WorktreeManager>,
        max_concurrent: usize,
        mcp_base_url: Option<String>,
    ) -> Self {
        Self {
            db,
            bus,
            engines: HashMap::new(),
            tiers: TierMapping::default(),
            worktrees,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            mcp_base_url,
            cancels: Mutex::new(HashMap::new()),
        }
    }

    pub fn register_engine(&mut self, engine: Arc<dyn Engine>) {
        self.engines.insert(engine.id(), engine);
    }

    pub fn engine(&self, id: &str) -> Option<Arc<dyn Engine>> {
        self.engines.get(id).cloned()
    }

    /// Create a run for a board task and put it on the queue.
    pub async fn enqueue_task(&self, task_id: Uuid) -> anyhow::Result<Uuid> {
        let row = sqlx::query(
            "INSERT INTO runs (task_id, status, trigger, engine)
             SELECT id, 'queued', 'manual', engine FROM tasks WHERE id = $1
             RETURNING id",
        )
        .bind(task_id)
        .fetch_one(&self.db.pool)
        .await?;
        let run_id: Uuid = row.get("id");
        sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, 10)")
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        Ok(run_id)
    }

    /// Create a run for a chat turn. Chat runs outrank task runs in the
    /// queue (priority 20 vs 10) so the assistant feels responsive.
    pub async fn enqueue_chat_turn(&self, chat_id: Uuid, engine: &str) -> anyhow::Result<Uuid> {
        let row = sqlx::query(
            "INSERT INTO runs (chat_id, status, trigger, engine)
             VALUES ($1, 'queued', 'chat', $2) RETURNING id",
        )
        .bind(chat_id)
        .bind(engine)
        .fetch_one(&self.db.pool)
        .await?;
        let run_id: Uuid = row.get("id");
        sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, 20)")
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        Ok(run_id)
    }

    pub fn cancel(&self, run_id: Uuid) {
        if let Some(tx) = self.cancels.lock().unwrap().remove(&run_id) {
            let _ = tx.send(());
        }
    }

    /// On boot: anything left in starting/running died with the previous
    /// process. Mark failed; the UI offers one-click resume via session_id.
    pub async fn recover_orphans(&self) -> anyhow::Result<u64> {
        let res = sqlx::query(
            "UPDATE runs SET status='failed', error_reason='orphaned by server restart',
             finished_at=now() WHERE status IN ('starting','running','waiting_permission')",
        )
        .execute(&self.db.pool)
        .await?;
        Ok(res.rows_affected())
    }

    /// Queue loop: claim ready runs under the concurrency semaphore.
    pub async fn run_loop(self: Arc<Self>) {
        loop {
            let permit = self.semaphore.clone().acquire_owned().await.expect("semaphore");
            match self.claim_next().await {
                Ok(Some(run_id)) => {
                    let this = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = this.execute(run_id).await {
                            tracing::error!(%run_id, error=%e, "run execution error");
                            let _ = this.finish(run_id, RunStatus::Failed, Some(e.to_string())).await;
                        }
                        drop(permit);
                    });
                }
                Ok(None) => {
                    drop(permit);
                    tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                }
                Err(e) => {
                    drop(permit);
                    tracing::error!(error=%e, "queue claim failed");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn claim_next(&self) -> anyhow::Result<Option<Uuid>> {
        let row = sqlx::query(
            "DELETE FROM queue WHERE run_id = (
                 SELECT run_id FROM queue
                 WHERE not_before IS NULL OR not_before <= now()
                 ORDER BY priority DESC, enqueued_at ASC
                 FOR UPDATE SKIP LOCKED LIMIT 1
             ) RETURNING run_id",
        )
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.map(|r| r.get("run_id")))
    }

    /// Dispatcher: task runs and chat runs share the streaming machinery but
    /// build their RunSpec differently.
    async fn execute(self: &Arc<Self>, run_id: Uuid) -> anyhow::Result<()> {
        let row = sqlx::query("SELECT chat_id FROM runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&self.db.pool)
            .await?;
        match row.get::<Option<Uuid>, _>("chat_id") {
            Some(chat_id) => self.execute_chat_run(run_id, chat_id).await,
            None => self.execute_task_run(run_id).await,
        }
    }

    async fn execute_task_run(self: &Arc<Self>, run_id: Uuid) -> anyhow::Result<()> {
        let run = sqlx::query(
            "SELECT r.id, r.engine, r.session_id, t.id AS task_id, t.prompt, t.model_tier,
                    t.permission_mode, t.title, t.worktree_path, t.branch, t.chat_id AS task_chat_id,
                    p.id AS project_id, p.path AS project_path, p.default_branch, p.full_auto_opt_in,
                    a.system_prompt AS agent_prompt, a.model_tier AS agent_tier,
                    a.allowed_tools AS agent_tools, a.permission_preset AS agent_preset,
                    a.name AS agent_name
             FROM runs r
             JOIN tasks t ON t.id = r.task_id
             JOIN projects p ON p.id = t.project_id
             LEFT JOIN agents a ON a.id = t.agent_id
             WHERE r.id = $1",
        )
        .bind(run_id)
        .fetch_one(&self.db.pool)
        .await?;

        self.set_status(run_id, RunStatus::Starting).await?;

        let engine_id: String = run.get("engine");
        let engine = self
            .engine(&engine_id)
            .ok_or_else(|| anyhow::anyhow!("unknown engine {engine_id}"))?;

        // Worktree: reuse the task's if it exists, else create one.
        let task_id: Uuid = run.get("task_id");
        let project_path = PathBuf::from(run.get::<String, _>("project_path"));
        let default_branch: String = run.get("default_branch");
        let cwd = match run.get::<Option<String>, _>("worktree_path") {
            Some(p) => PathBuf::from(p),
            None => {
                let title: String = run.get("title");
                let wt = self
                    .worktrees
                    .create(&project_path, &default_branch, task_id, &slugify(&title))
                    .await?;
                sqlx::query("UPDATE tasks SET worktree_path=$1, branch=$2 WHERE id=$3")
                    .bind(wt.path.to_string_lossy().as_ref())
                    .bind(&wt.branch)
                    .bind(task_id)
                    .execute(&self.db.pool)
                    .await?;
                wt.path
            }
        };

        // Agent binding: a bound agent overrides tier, system prompt,
        // allowed tools, and permission preset.
        let agent_prompt: Option<String> = run.get("agent_prompt");
        let agent_tier: Option<String> = run.get("agent_tier");
        let agent_tools: Option<Vec<String>> = run.get("agent_tools");
        let agent_preset: Option<String> = run.get("agent_preset");

        let tier_str: String = agent_tier.unwrap_or_else(|| run.get("model_tier"));
        let tier: ModelTier =
            serde_json::from_value(serde_json::Value::String(tier_str)).unwrap_or_default();
        let mode_str: String = agent_preset.unwrap_or_else(|| run.get("permission_mode"));
        let permission_mode: PermissionMode =
            serde_json::from_value(serde_json::Value::String(mode_str)).unwrap_or_default();

        // FullAuto is refused outside aichip-managed worktrees and outside
        // opted-in projects — the structural safety gate.
        let full_auto_opt_in: bool = run.get("full_auto_opt_in");
        let permission_mode = if permission_mode == PermissionMode::FullAuto
            && !(full_auto_opt_in && self.worktrees.manages(&cwd))
        {
            tracing::warn!(%run_id, "downgrading FullAuto to Reviewed (gate not satisfied)");
            PermissionMode::Reviewed
        } else {
            permission_mode
        };

        let mcp_config_path = match (&self.mcp_base_url, engine_id.as_str()) {
            (Some(base), "claude-code") => {
                Some(write_mcp_config(base, &format!("mcp/run/{run_id}"), run_id).await?)
            }
            _ => None,
        };

        let model_id = self.tiers.model_for(tier).to_string();
        sqlx::query("UPDATE runs SET model=$1, started_at=now() WHERE id=$2")
            .bind(&model_id)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;

        let spec = RunSpec {
            cwd,
            prompt: run.get("prompt"),
            model_tier: tier,
            model_id,
            resume_session_id: run.get("session_id"),
            permission_mode,
            allowed_tools: agent_tools.unwrap_or_default(),
            append_system_prompt: agent_prompt.filter(|p| !p.is_empty()),
            mcp_config_path,
            permission_prompt_tool: true,
            extra_env: HashMap::from([
                ("AICHIP_RUN_ID".to_string(), run_id.to_string()),
                // Permission prompts block the MCP tools/call until the user
                // answers in the dashboard; raise the CLI's tool timeout so
                // it waits as long as the broker does (15 min).
                ("MCP_TOOL_TIMEOUT".to_string(), "900000".to_string()),
                ("MCP_TIMEOUT".to_string(), "60000".to_string()),
            ]),
        };

        let outcome = self.stream_run(run_id, engine, spec).await?;

        if outcome.status == RunStatus::Completed {
            sqlx::query("UPDATE tasks SET board_column='review' WHERE id=$1")
                .bind(task_id)
                .execute(&self.db.pool)
                .await?;
        }

        // A task spawned from chat reports back into that chat.
        if let Some(chat_id) = run.get::<Option<Uuid>, _>("task_chat_id") {
            if outcome.status.is_terminal() {
                let title: String = run.get("title");
                let note = match outcome.status {
                    RunStatus::Completed => {
                        format!("Task \"{title}\" completed — ready for review on the board.")
                    }
                    RunStatus::Canceled => format!("Task \"{title}\" was canceled."),
                    _ => format!(
                        "Task \"{title}\" failed: {}",
                        outcome.reason.clone().unwrap_or_default()
                    ),
                };
                let _ = sqlx::query(
                    "INSERT INTO chat_messages (chat_id, role, content, run_id)
                     VALUES ($1, 'system', $2, $3)",
                )
                .bind(chat_id)
                .bind(note)
                .bind(run_id)
                .execute(&self.db.pool)
                .await;
            }
        }
        Ok(())
    }

    async fn execute_chat_run(self: &Arc<Self>, run_id: Uuid, chat_id: Uuid) -> anyhow::Result<()> {
        let row = sqlx::query(
            "SELECT r.engine, c.session_id, c.session_engine, p.path AS project_path,
                    (SELECT content FROM chat_messages
                     WHERE chat_id = c.id AND role = 'user'
                     ORDER BY created_at DESC LIMIT 1) AS user_message
             FROM runs r JOIN chats c ON c.id = r.chat_id
             JOIN projects p ON p.id = c.project_id
             WHERE r.id = $1",
        )
        .bind(run_id)
        .fetch_one(&self.db.pool)
        .await?;

        self.set_status(run_id, RunStatus::Starting).await?;

        let engine_id: String = row.get("engine");
        let engine = self
            .engine(&engine_id)
            .ok_or_else(|| anyhow::anyhow!("unknown engine {engine_id}"))?;

        // A session is only resumable by the engine that created it — a
        // mock-produced session id would make the real CLI fail instantly.
        let session_id: Option<String> = row
            .get::<Option<String>, _>("session_id")
            .filter(|_| row.get::<Option<String>, _>("session_engine").as_deref() == Some(&engine_id));
        let user_message: String = row
            .get::<Option<String>, _>("user_message")
            .unwrap_or_else(|| "Introduce yourself briefly.".to_string());

        let mcp_config_path = match (&self.mcp_base_url, engine_id.as_str()) {
            (Some(base), "claude-code") => {
                Some(write_mcp_config(base, &format!("mcp/chat/{chat_id}"), run_id).await?)
            }
            _ => None,
        };

        let tier = ModelTier::Medium;
        let model_id = self.tiers.model_for(tier).to_string();
        sqlx::query("UPDATE runs SET model=$1, started_at=now() WHERE id=$2")
            .bind(&model_id)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;

        let spec = RunSpec {
            cwd: PathBuf::from(row.get::<String, _>("project_path")),
            prompt: user_message,
            model_tier: tier,
            model_id,
            resume_session_id: session_id,
            permission_mode: PermissionMode::Reviewed,
            allowed_tools: CHAT_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect(),
            append_system_prompt: Some(CHAT_SYSTEM_PROMPT.to_string()),
            mcp_config_path,
            permission_prompt_tool: false,
            extra_env: HashMap::from([("AICHIP_CHAT_ID".to_string(), chat_id.to_string())]),
        };

        let outcome = self.stream_run(run_id, engine, spec).await?;

        if outcome.status == RunStatus::Completed {
            // Persist the (forked) session id for the next --resume, and the
            // assistant's reply as a durable chat message.
            let final_row = sqlx::query("SELECT session_id FROM runs WHERE id=$1")
                .bind(run_id)
                .fetch_one(&self.db.pool)
                .await?;
            if let Some(sid) = final_row.get::<Option<String>, _>("session_id") {
                sqlx::query(
                    "UPDATE chats SET session_id=$1, session_engine=$2, updated_at=now() WHERE id=$3",
                )
                .bind(sid)
                .bind(&engine_id)
                .bind(chat_id)
                .execute(&self.db.pool)
                .await?;
            }
            let reply: Option<String> = sqlx::query(
                "SELECT payload->>'result_text' AS text FROM events
                 WHERE run_id=$1 AND type='run_completed' ORDER BY seq DESC LIMIT 1",
            )
            .bind(run_id)
            .fetch_optional(&self.db.pool)
            .await?
            .and_then(|r| r.get::<Option<String>, _>("text"));
            sqlx::query(
                "INSERT INTO chat_messages (chat_id, role, content, run_id)
                 VALUES ($1, 'assistant', $2, $3)",
            )
            .bind(chat_id)
            .bind(reply.filter(|t| !t.is_empty()).unwrap_or_else(|| "(no reply)".into()))
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        } else if outcome.status == RunStatus::Failed {
            sqlx::query(
                "INSERT INTO chat_messages (chat_id, role, content, run_id)
                 VALUES ($1, 'system', $2, $3)",
            )
            .bind(chat_id)
            .bind(format!(
                "Assistant turn failed: {}",
                outcome.reason.clone().unwrap_or_default()
            ))
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        }
        Ok(())
    }

    /// Shared streaming loop: spawn the engine, persist+publish every event,
    /// handle cancel / rate-limit / terminal transitions.
    async fn stream_run(
        self: &Arc<Self>,
        run_id: Uuid,
        engine: Arc<dyn Engine>,
        spec: RunSpec,
    ) -> anyhow::Result<StreamOutcome> {
        let mut proc = engine.start(spec)?;
        self.set_status(run_id, RunStatus::Running).await?;

        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.cancels.lock().unwrap().insert(run_id, cancel_tx);

        let mut seq: i64 = next_seq(&self.db, run_id).await?;
        let mut outcome: Option<(RunStatus, Option<String>)> = None;
        loop {
            tokio::select! {
                _ = &mut cancel_rx => {
                    let _ = proc.interrupt().await;
                    outcome = Some((RunStatus::Canceled, None));
                    break;
                }
                event = proc.events.recv() => {
                    let Some(event) = event else { break };
                    self.persist_and_publish(run_id, seq, &event).await?;
                    seq += 1;
                    match &event {
                        AichipEvent::RunStarted { session_id, .. } => {
                            if let Some(sid) = session_id {
                                sqlx::query("UPDATE runs SET session_id=$1 WHERE id=$2")
                                    .bind(sid).bind(run_id)
                                    .execute(&self.db.pool).await?;
                            }
                        }
                        AichipEvent::RunCompleted { session_id, cost_usd, usage, .. } => {
                            sqlx::query(
                                "UPDATE runs SET session_id=$1, cost_usd=$2,
                                 input_tokens=$3, output_tokens=$4 WHERE id=$5")
                                .bind(session_id)
                                .bind(cost_usd)
                                .bind(usage.input_tokens as i64)
                                .bind(usage.output_tokens as i64)
                                .bind(run_id)
                                .execute(&self.db.pool).await?;
                            outcome = Some((RunStatus::Completed, None));
                        }
                        AichipEvent::RunFailed { reason } => {
                            outcome = Some((RunStatus::Failed, Some(reason.clone())));
                        }
                        AichipEvent::RateLimited { reset_at, message } => {
                            self.requeue_rate_limited(run_id, *reset_at).await?;
                            outcome = Some((RunStatus::RateLimited, Some(message.clone())));
                        }
                        _ => {}
                    }
                }
            }
        }
        self.cancels.lock().unwrap().remove(&run_id);

        let (status, reason) =
            outcome.unwrap_or((RunStatus::Failed, Some("event stream ended unexpectedly".into())));
        if status != RunStatus::RateLimited {
            self.finish(run_id, status, reason.clone()).await?;
        } else {
            self.set_status(run_id, RunStatus::RateLimited).await?;
        }
        Ok(StreamOutcome { status, reason })
    }

    async fn requeue_rate_limited(
        &self,
        run_id: Uuid,
        reset_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        let not_before = rate_limit_backoff(0, reset_at);
        sqlx::query(
            "INSERT INTO queue (run_id, priority, not_before) VALUES ($1, 5, $2)
             ON CONFLICT (run_id) DO UPDATE SET not_before = EXCLUDED.not_before",
        )
        .bind(run_id)
        .bind(not_before)
        .execute(&self.db.pool)
        .await?;
        Ok(())
    }

    async fn set_status(&self, run_id: Uuid, status: RunStatus) -> anyhow::Result<()> {
        sqlx::query("UPDATE runs SET status=$1 WHERE id=$2")
            .bind(status.as_str())
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        Ok(())
    }

    async fn finish(
        &self,
        run_id: Uuid,
        status: RunStatus,
        reason: Option<String>,
    ) -> anyhow::Result<()> {
        sqlx::query("UPDATE runs SET status=$1, error_reason=$2, finished_at=now() WHERE id=$3")
            .bind(status.as_str())
            .bind(reason)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        Ok(())
    }

    async fn persist_and_publish(
        &self,
        run_id: Uuid,
        seq: i64,
        event: &AichipEvent,
    ) -> anyhow::Result<()> {
        let payload = serde_json::to_value(event)?;
        let type_name = payload
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let ts: DateTime<Utc> = Utc::now();
        sqlx::query(
            "INSERT INTO events (run_id, seq, type, payload, ts) VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(run_id)
        .bind(seq)
        .bind(&type_name)
        .bind(&payload)
        .bind(ts)
        .execute(&self.db.pool)
        .await?;
        self.bus.publish(EventEnvelope {
            run_id,
            step_id: None,
            seq,
            ts,
            event: event.clone(),
        });
        Ok(())
    }
}

async fn next_seq(db: &Db, run_id: Uuid) -> anyhow::Result<i64> {
    let row = sqlx::query("SELECT COALESCE(MAX(seq), -1) + 1 AS next FROM events WHERE run_id=$1")
        .bind(run_id)
        .fetch_one(&db.pool)
        .await?;
    Ok(row.get("next"))
}

/// Write a per-run MCP config pointing the engine at one of aichip's MCP
/// endpoints. Lives under ~/.aichip/mcp/, keyed by run id.
async fn write_mcp_config(base_url: &str, url_path: &str, run_id: Uuid) -> anyhow::Result<PathBuf> {
    let dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aichip")
        .join("mcp");
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("{run_id}.json"));
    let config = serde_json::json!({
        "mcpServers": {
            "aichip": { "type": "http", "url": format!("{base_url}/{url_path}") }
        }
    });
    tokio::fs::write(&path, serde_json::to_vec_pretty(&config)?).await?;
    Ok(path)
}

fn slugify(s: &str) -> String {
    let slug: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    slug.chars().take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_is_branch_safe() {
        assert_eq!(slugify("Fix: the (weird) bug!!"), "fix-the-weird-bug");
        assert!(slugify(&"x".repeat(100)).len() <= 40);
    }
}
