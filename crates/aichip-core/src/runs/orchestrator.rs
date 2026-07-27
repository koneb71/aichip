//! Run orchestrator: owns the run state machine, the concurrency semaphore,
//! and the queue loop. All status transitions happen here and every event is
//! persisted before it is broadcast, so the DB is the source of truth.
//!
//! Two kinds of runs share the same machinery:
//! - task runs (board cards): isolated worktree, permission proxy
//! - chat runs (project assistant turns): real checkout cwd, but locked to a
//!   read-only + aichip-tools allowed list — never Bash/Edit/Write there.

use aichip_shared::workflow::{SessionMode, StepOutputs, Workflow};
use aichip_shared::{AichipEvent, EventEnvelope, ModelTier, PermissionMode, RunStatus, TierMapping};
use aichip_engines::{Engine, RunSpec};
use chrono::{DateTime, Utc};
use futures::StreamExt;
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
    /// Final assistant/result text, used to feed later pipeline steps.
    output: String,
    session_id: Option<String>,
}

/// An agent from the library, resolved for a step or task.
struct BoundAgent {
    system_prompt: String,
    tier: ModelTier,
    allowed_tools: Vec<String>,
    permission_preset: String,
}

/// Race-free `events.seq` allocation. A workflow run has several steps
/// writing concurrently, and `(run_id, seq)` is unique.
#[derive(Clone)]
struct SeqAlloc(Arc<std::sync::atomic::AtomicI64>);

impl SeqAlloc {
    fn starting_at(seq: i64) -> Self {
        Self(Arc::new(std::sync::atomic::AtomicI64::new(seq)))
    }
    fn next(&self) -> i64 {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
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

    /// Queue a workflow execution. `trigger` distinguishes manual runs from
    /// scheduled ones for the activity view.
    pub async fn enqueue_workflow(
        &self,
        workflow_id: Uuid,
        trigger: &str,
    ) -> anyhow::Result<Uuid> {
        let row = sqlx::query(
            "INSERT INTO runs (workflow_id, status, trigger, engine)
             VALUES ($1, 'queued', $2, 'claude-code') RETURNING id",
        )
        .bind(workflow_id)
        .bind(trigger)
        .fetch_one(&self.db.pool)
        .await?;
        let run_id: Uuid = row.get("id");
        // Scheduled work yields to anything a human is waiting on.
        let priority = if trigger == "schedule" { 1 } else { 10 };
        sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, $2)")
            .bind(run_id)
            .bind(priority)
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
        let row = sqlx::query("SELECT chat_id, workflow_id FROM runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&self.db.pool)
            .await?;
        match (
            row.get::<Option<Uuid>, _>("chat_id"),
            row.get::<Option<Uuid>, _>("workflow_id"),
        ) {
            (Some(chat_id), _) => self.execute_chat_run(run_id, chat_id).await,
            (_, Some(workflow_id)) => self.execute_workflow_run(run_id, workflow_id).await,
            _ => self.execute_task_run(run_id).await,
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

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let outcome = self
            .stream_run(run_id, None, &seq, engine, spec, true)
            .await?;

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

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let outcome = self
            .stream_run(run_id, None, &seq, engine, spec, true)
            .await?;

        if outcome.status == RunStatus::Completed {
            // Persist the (forked) session id for the next --resume, and the
            // assistant's reply as a durable chat message.
            if let Some(sid) = &outcome.session_id {
                sqlx::query(
                    "UPDATE chats SET session_id=$1, session_engine=$2, updated_at=now() WHERE id=$3",
                )
                .bind(sid)
                .bind(&engine_id)
                .bind(chat_id)
                .execute(&self.db.pool)
                .await?;
            }
            sqlx::query(
                "INSERT INTO chat_messages (chat_id, role, content, run_id)
                 VALUES ($1, 'assistant', $2, $3)",
            )
            .bind(chat_id)
            .bind(if outcome.output.is_empty() {
                "(no reply)".to_string()
            } else {
                outcome.output.clone()
            })
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

    /// Execute a workflow: steps run in dependency order, a step's outputs
    /// feed later prompts, and `strategy.parallel` fans a step out into
    /// independent attempts. The whole workflow is one run, so the existing
    /// streaming, replay, and cost tracking apply unchanged.
    async fn execute_workflow_run(
        self: &Arc<Self>,
        run_id: Uuid,
        workflow_id: Uuid,
    ) -> anyhow::Result<()> {
        let row = sqlx::query(
            "SELECT w.source_yaml, w.name, r.engine, p.id AS project_id, p.workspace_id,
                    p.path AS project_path, p.default_branch, p.full_auto_opt_in
             FROM runs r JOIN workflows w ON w.id = r.workflow_id
             JOIN projects p ON p.id = w.project_id
             WHERE r.id = $1",
        )
        .bind(run_id)
        .fetch_one(&self.db.pool)
        .await?;

        self.set_status(run_id, RunStatus::Starting).await?;

        let workflow = Workflow::from_yaml(&row.get::<String, _>("source_yaml"))?;
        // The YAML's `defaults.engine` wins: a scheduled run is queued long
        // before we know which engine the workflow asked for.
        let engine_id = workflow.defaults.engine.clone();
        let engine = self
            .engine(&engine_id)
            .ok_or_else(|| anyhow::anyhow!("unknown engine {engine_id}"))?;
        sqlx::query("UPDATE runs SET engine=$1 WHERE id=$2")
            .bind(&engine_id)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        let project_path = PathBuf::from(row.get::<String, _>("project_path"));
        let default_branch: String = row.get("default_branch");
        let workspace_id: Uuid = row.get("workspace_id");
        let full_auto_opt_in: bool = row.get("full_auto_opt_in");

        // Shared worktree: sequential stages build on each other's work.
        // Fan-out steps that ask for isolation get their own.
        let shared = self
            .worktrees
            .create(
                &project_path,
                &default_branch,
                run_id,
                &slugify(&workflow.name),
            )
            .await?;

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let mut outputs = StepOutputs::new();
        let mut sessions: HashMap<String, String> = HashMap::new();
        let mut failure: Option<String> = None;

        'layers: for layer in workflow.layers()? {
            for step_id in layer {
                let step = workflow
                    .step(&step_id)
                    .ok_or_else(|| anyhow::anyhow!("missing step {step_id}"))?;

                let agent = self.load_agent(workspace_id, step.agent.as_deref()).await?;
                let model_id = workflow.resolve_model(step, agent.as_ref().map(|a| a.tier), &self.tiers);
                let prompt = aichip_shared::interpolate(&step.prompt, &outputs);

                let permission_mode = self.workflow_permission_mode(
                    &workflow,
                    agent.as_ref(),
                    full_auto_opt_in,
                    &shared.path,
                );

                // Resume the session of the step we depend on, so a
                // "continue" stage keeps the prior stage's context.
                let resume = if step.session == SessionMode::Continue {
                    step.needs.first().and_then(|n| sessions.get(n).cloned())
                } else {
                    None
                };

                let attempts = step.parallelism();
                let mut plans = Vec::with_capacity(attempts);
                for index in 0..attempts {
                    let step_key = if attempts > 1 {
                        format!("{step_id}#{}", index + 1)
                    } else {
                        step_id.clone()
                    };
                    let cwd = if step.isolated_worktrees() && attempts > 1 {
                        self.worktrees
                            .create(
                                &project_path,
                                &default_branch,
                                Uuid::new_v4(),
                                &format!("{}-{}", slugify(&step_key), index + 1),
                            )
                            .await?
                            .path
                    } else {
                        shared.path.clone()
                    };
                    let db_step_id = self.create_step_row(run_id, &step_key).await?;
                    plans.push((db_step_id, step_key, cwd));
                }

                // Opportunistic parallelism: this run already holds one queue
                // permit; extra ones are taken only if immediately free, so a
                // fan-out can never deadlock against another workflow.
                let extra: Vec<_> = (1..plans.len())
                    .filter_map(|_| self.semaphore.clone().try_acquire_owned().ok())
                    .collect();
                let concurrency = 1 + extra.len();

                let results = futures::stream::iter(plans.into_iter().map(
                    |(db_step_id, step_key, cwd)| {
                        let this = self.clone();
                        let engine = engine.clone();
                        let seq = seq.clone();
                        let spec = RunSpec {
                            cwd,
                            prompt: prompt.clone(),
                            model_tier: agent.as_ref().map(|a| a.tier).unwrap_or_default(),
                            model_id: model_id.clone(),
                            resume_session_id: resume.clone(),
                            permission_mode,
                            allowed_tools: agent
                                .as_ref()
                                .map(|a| a.allowed_tools.clone())
                                .unwrap_or_default(),
                            append_system_prompt: agent
                                .as_ref()
                                .and_then(|a| (!a.system_prompt.is_empty()).then(|| a.system_prompt.clone())),
                            mcp_config_path: None,
                            permission_prompt_tool: true,
                            extra_env: HashMap::from([
                                ("AICHIP_RUN_ID".to_string(), run_id.to_string()),
                                ("AICHIP_STEP".to_string(), step_key.clone()),
                                ("MCP_TOOL_TIMEOUT".to_string(), "900000".to_string()),
                            ]),
                        };
                        async move {
                            let outcome = this
                                .stream_run(run_id, Some(db_step_id), &seq, engine, spec, false)
                                .await;
                            (db_step_id, step_key, outcome)
                        }
                    },
                ))
                .buffer_unordered(concurrency)
                .collect::<Vec<_>>()
                .await;
                drop(extra);

                let mut step_outputs = Vec::with_capacity(results.len());
                for (db_step_id, step_key, outcome) in results {
                    let outcome = outcome?;
                    self.finish_step_row(db_step_id, &outcome).await?;
                    if outcome.status != RunStatus::Completed {
                        failure = Some(format!(
                            "step '{step_key}' {}: {}",
                            outcome.status.as_str(),
                            outcome.reason.clone().unwrap_or_default()
                        ));
                        break 'layers;
                    }
                    if let Some(sid) = outcome.session_id.clone() {
                        sessions.entry(step_id.clone()).or_insert(sid);
                    }
                    step_outputs.push(outcome.output);
                }
                outputs.insert(step_id.clone(), step_outputs);
            }
        }

        match failure {
            None => {
                self.finish(run_id, RunStatus::Completed, None).await?;
                // Surface the pipeline's result the same way a task does.
                sqlx::query(
                    "UPDATE workflows SET last_run_at = now() WHERE id = $1",
                )
                .bind(workflow_id)
                .execute(&self.db.pool)
                .await?;
            }
            Some(reason) => {
                self.finish(run_id, RunStatus::Failed, Some(reason)).await?;
            }
        }
        Ok(())
    }

    async fn create_step_row(&self, run_id: Uuid, step_key: &str) -> anyhow::Result<Uuid> {
        let row = sqlx::query(
            "INSERT INTO steps (run_id, step_key, status, started_at)
             VALUES ($1, $2, 'running', now()) RETURNING id",
        )
        .bind(run_id)
        .bind(step_key)
        .fetch_one(&self.db.pool)
        .await?;
        Ok(row.get("id"))
    }

    async fn finish_step_row(
        &self,
        step_id: Uuid,
        outcome: &StreamOutcome,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE steps SET status=$1, session_id=$2, output_text=$3, finished_at=now()
             WHERE id=$4",
        )
        .bind(outcome.status.as_str())
        .bind(&outcome.session_id)
        .bind(&outcome.output)
        .bind(step_id)
        .execute(&self.db.pool)
        .await?;
        Ok(())
    }

    async fn load_agent(
        &self,
        workspace_id: Uuid,
        name: Option<&str>,
    ) -> anyhow::Result<Option<BoundAgent>> {
        let Some(name) = name else { return Ok(None) };
        let row = sqlx::query(
            "SELECT system_prompt, model_tier, allowed_tools, permission_preset
             FROM agents WHERE workspace_id=$1 AND name=$2",
        )
        .bind(workspace_id)
        .bind(name)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.map(|r| BoundAgent {
            system_prompt: r.get("system_prompt"),
            tier: serde_json::from_value(serde_json::Value::String(r.get("model_tier")))
                .unwrap_or_default(),
            allowed_tools: r.get("allowed_tools"),
            permission_preset: r.get("permission_preset"),
        }))
    }

    /// Workflow steps default to auto-edit (they run unattended in a
    /// worktree); FullAuto still requires the project opt-in and a managed
    /// worktree, same gate as task runs.
    fn workflow_permission_mode(
        &self,
        workflow: &Workflow,
        agent: Option<&BoundAgent>,
        full_auto_opt_in: bool,
        cwd: &std::path::Path,
    ) -> PermissionMode {
        let spec = agent
            .map(|a| a.permission_preset.clone())
            .or_else(|| workflow.defaults.permission_mode.clone());
        let mode = spec
            .and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok())
            .unwrap_or(PermissionMode::AutoEdit);
        if mode == PermissionMode::FullAuto && !(full_auto_opt_in && self.worktrees.manages(cwd)) {
            PermissionMode::Reviewed
        } else {
            mode
        }
    }

    /// Shared streaming loop: spawn the engine, persist+publish every event,
    /// handle cancel / rate-limit / terminal transitions.
    ///
    /// `step_id` tags events for pipeline steps. `finalize` is false for
    /// individual workflow steps — the workflow decides the run's final
    /// status once every step is done.
    async fn stream_run(
        self: &Arc<Self>,
        run_id: Uuid,
        step_id: Option<Uuid>,
        seq: &SeqAlloc,
        engine: Arc<dyn Engine>,
        spec: RunSpec,
        finalize: bool,
    ) -> anyhow::Result<StreamOutcome> {
        let mut proc = engine.start(spec)?;
        self.set_status(run_id, RunStatus::Running).await?;

        // A cancel targets the whole run; steps register under their own id
        // so a workflow's parallel steps don't clobber each other's channel.
        let cancel_key = step_id.unwrap_or(run_id);
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.cancels.lock().unwrap().insert(cancel_key, cancel_tx);

        let mut outcome: Option<(RunStatus, Option<String>)> = None;
        let mut text_parts: Vec<String> = vec![];
        let mut result_text = String::new();
        let mut session_id: Option<String> = None;
        loop {
            tokio::select! {
                _ = &mut cancel_rx => {
                    let _ = proc.interrupt().await;
                    outcome = Some((RunStatus::Canceled, None));
                    break;
                }
                event = proc.events.recv() => {
                    let Some(event) = event else { break };
                    self.persist_and_publish(run_id, step_id, seq.next(), &event).await?;
                    match &event {
                        AichipEvent::AssistantText { text } => text_parts.push(text.clone()),
                        AichipEvent::RunStarted { session_id: sid, .. } => {
                            if let Some(sid) = sid {
                                session_id = Some(sid.clone());
                                sqlx::query("UPDATE runs SET session_id=$1 WHERE id=$2")
                                    .bind(sid).bind(run_id)
                                    .execute(&self.db.pool).await?;
                            }
                        }
                        AichipEvent::RunCompleted { session_id: sid, cost_usd, usage, result_text: rt } => {
                            session_id = Some(sid.clone());
                            result_text = rt.clone();
                            // Costs accumulate: a workflow run has many steps.
                            sqlx::query(
                                "UPDATE runs SET session_id=$1,
                                 cost_usd = COALESCE(cost_usd, 0) + COALESCE($2, 0),
                                 input_tokens = input_tokens + $3,
                                 output_tokens = output_tokens + $4 WHERE id=$5")
                                .bind(sid)
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
        self.cancels.lock().unwrap().remove(&cancel_key);

        let (status, reason) =
            outcome.unwrap_or((RunStatus::Failed, Some("event stream ended unexpectedly".into())));
        if finalize {
            if status != RunStatus::RateLimited {
                self.finish(run_id, status, reason.clone()).await?;
            } else {
                self.set_status(run_id, RunStatus::RateLimited).await?;
            }
        }
        let output = if result_text.is_empty() {
            text_parts.join("\n")
        } else {
            result_text
        };
        Ok(StreamOutcome {
            status,
            reason,
            output,
            session_id,
        })
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
        step_id: Option<Uuid>,
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
            "INSERT INTO events (run_id, step_id, seq, type, payload, ts)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(run_id)
        .bind(step_id)
        .bind(seq)
        .bind(&type_name)
        .bind(&payload)
        .bind(ts)
        .execute(&self.db.pool)
        .await?;
        self.bus.publish(EventEnvelope {
            run_id,
            step_id,
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
