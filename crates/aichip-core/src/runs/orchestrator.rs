//! Run orchestrator: owns the run state machine, the concurrency semaphore,
//! and the queue loop. All status transitions happen here and every event is
//! persisted before it is broadcast, so the DB is the source of truth.
//!
//! Two kinds of runs share the same machinery:
//! - task runs (board cards): isolated worktree, permission proxy
//! - chat runs (project assistant turns): real checkout cwd, but locked to a
//!   read-only + aichip-tools allowed list — never Bash/Edit/Write there.

use aichip_shared::workflow::{SessionMode, StepOutputs, Workflow};
use aichip_shared::{
    AichipEvent, EventEnvelope, ModelTier, PermissionMode, ReasoningEffort, RunStatus,
    TierMapping,
};
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
use crate::runs::attachments;
use crate::runs::memory;
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
    pub(crate) semaphore: Arc<Semaphore>,
    /// Base URL of aichip's MCP endpoints, e.g. "http://127.0.0.1:4820".
    /// None disables MCP wiring (mock engine / tests).
    pub(crate) mcp_base_url: Option<String>,
    cancels: Mutex<HashMap<Uuid, oneshot::Sender<()>>>,
}

pub(crate) struct StreamOutcome {
    pub status: RunStatus,
    pub reason: Option<String>,
    /// Final assistant/result text, used to feed later pipeline steps.
    pub output: String,
    pub session_id: Option<String>,
}

/// An agent from the library, resolved for a step or task.
pub(crate) struct BoundAgent {
    pub system_prompt: String,
    pub tier: ModelTier,
    pub effort: Option<ReasoningEffort>,
    pub allowed_tools: Vec<String>,
    pub permission_preset: String,
}

/// Race-free `events.seq` allocation. A workflow run has several steps
/// writing concurrently, and `(run_id, seq)` is unique.
#[derive(Clone)]
pub(crate) struct SeqAlloc(Arc<std::sync::atomic::AtomicI64>);

impl SeqAlloc {
    pub(crate) fn starting_at(seq: i64) -> Self {
        Self(Arc::new(std::sync::atomic::AtomicI64::new(seq)))
    }
    pub(crate) fn next(&self) -> i64 {
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

    /// Create a run for a board task and put it on the queue. A task handed
    /// to a team runs as that team instead of a single agent.
    pub async fn enqueue_task(&self, task_id: Uuid) -> anyhow::Result<Uuid> {
        let assigned_team: Option<Uuid> = sqlx::query("SELECT team_id FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&self.db.pool)
            .await?
            .get("team_id");
        if let Some(team_id) = assigned_team {
            return self.enqueue_task_for_team(task_id, team_id).await;
        }

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

    /// The worktree a run should work in. Team runs launched from a board
    /// task reuse that task's worktree, so the existing review-and-merge
    /// flow keeps working no matter who did the work.
    pub(crate) async fn worktree_for_run(
        &self,
        run_id: Uuid,
        task_id: Option<Uuid>,
        project_path: &std::path::Path,
        default_branch: &str,
        slug: &str,
    ) -> anyhow::Result<crate::worktrees::manager::Worktree> {
        // Looked up here rather than passed in: every caller would otherwise
        // have to remember, and forgetting means `git worktree add` against a
        // folder with no repository.
        if self.is_in_place(project_path).await? {
            return Ok(crate::worktrees::manager::Worktree {
                path: project_path.to_path_buf(),
                // Empty signals "no branch to merge"; nothing is persisted to
                // the task, so diff/merge stay unavailable.
                branch: String::new(),
            });
        }
        if let Some(task_id) = task_id {
            let row = sqlx::query("SELECT worktree_path, branch FROM tasks WHERE id = $1")
                .bind(task_id)
                .fetch_one(&self.db.pool)
                .await?;
            if let (Some(path), Some(branch)) = (
                row.get::<Option<String>, _>("worktree_path"),
                row.get::<Option<String>, _>("branch"),
            ) {
                return Ok(crate::worktrees::manager::Worktree {
                    path: PathBuf::from(path),
                    branch,
                });
            }
            let worktree = self
                .worktrees
                .create(project_path, default_branch, task_id, slug)
                .await?;
            sqlx::query("UPDATE tasks SET worktree_path=$1, branch=$2 WHERE id=$3")
                .bind(worktree.path.to_string_lossy().as_ref())
                .bind(&worktree.branch)
                .bind(task_id)
                .execute(&self.db.pool)
                .await?;
            return Ok(worktree);
        }
        self.worktrees
            .create(project_path, default_branch, run_id, slug)
            .await
    }

    /// True when a project has no version control, so its runs happen in the
    /// folder itself instead of an isolated worktree.
    pub(crate) async fn is_in_place(&self, project_path: &std::path::Path) -> anyhow::Result<bool> {
        let vcs: Option<(String,)> = sqlx::query_as("SELECT vcs FROM projects WHERE path = $1")
            .bind(project_path.to_string_lossy().as_ref())
            .fetch_optional(&self.db.pool)
            .await?;
        Ok(matches!(vcs, Some((v,)) if v != "git"))
    }

    /// Move a team-run task onto its landing column when the run finishes:
    /// review when there is a diff to look at, done when there isn't.
    pub(crate) async fn settle_task_for_run(
        &self,
        task_id: Option<Uuid>,
        status: RunStatus,
    ) -> anyhow::Result<()> {
        let Some(task_id) = task_id else { return Ok(()) };
        if status == RunStatus::Completed {
            sqlx::query(
                "UPDATE tasks t SET board_column = CASE WHEN p.vcs = 'git' THEN 'review' ELSE 'done' END
                 FROM projects p WHERE p.id = t.project_id AND t.id = $1",
            )
            .bind(task_id)
            .execute(&self.db.pool)
            .await?;
        }
        Ok(())
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

    /// Queue an agent's reply to an @-mention in a task comment. Priority 15:
    /// a mentioned agent should feel responsive, but the user's own chat
    /// turns (20) still come first.
    pub async fn enqueue_comment_reply(
        &self,
        comment_id: Uuid,
        agent_id: Uuid,
        engine: &str,
    ) -> anyhow::Result<Uuid> {
        let row = sqlx::query(
            "INSERT INTO runs (comment_id, agent_id, status, trigger, engine)
             VALUES ($1, $2, 'queued', 'comment', $3) RETURNING id",
        )
        .bind(comment_id)
        .bind(agent_id)
        .bind(engine)
        .fetch_one(&self.db.pool)
        .await?;
        let run_id: Uuid = row.get("id");
        sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, 15)")
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
        let row = sqlx::query("SELECT chat_id, workflow_id, team_id, comment_id FROM runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&self.db.pool)
            .await?;
        match (
            row.get::<Option<Uuid>, _>("chat_id"),
            row.get::<Option<Uuid>, _>("workflow_id"),
            row.get::<Option<Uuid>, _>("team_id"),
            row.get::<Option<Uuid>, _>("comment_id"),
        ) {
            (Some(chat_id), _, _, _) => self.execute_chat_run(run_id, chat_id).await,
            (_, Some(workflow_id), _, _) => self.execute_workflow_run(run_id, workflow_id).await,
            (_, _, Some(team_id), _) => self.execute_org_run(run_id, team_id).await,
            (_, _, _, Some(comment_id)) => self.execute_comment_run(run_id, comment_id).await,
            _ => self.execute_task_run(run_id).await,
        }
    }

    async fn execute_task_run(self: &Arc<Self>, run_id: Uuid) -> anyhow::Result<()> {
        let run = sqlx::query(
            "SELECT r.id, r.engine, r.session_id, t.id AS task_id, t.prompt, t.model_tier,
                    t.agent_id,
                    t.permission_mode, t.title, t.worktree_path, t.branch, t.chat_id AS task_chat_id,
                    p.id AS project_id, p.path AS project_path, p.default_branch, p.full_auto_opt_in,
                    p.vcs,
                    a.system_prompt AS agent_prompt, a.model_tier AS agent_tier,
                    a.effort AS agent_effort,
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
        // A project without version control has nowhere to branch from, so the
        // run happens in the folder itself. That trades away isolation and the
        // review step; `settle` below sends the task straight to done because
        // there is no diff to look at.
        let in_place = run.get::<String, _>("vcs") != "git";
        let cwd = match run.get::<Option<String>, _>("worktree_path") {
            Some(p) => PathBuf::from(p),
            None if in_place => project_path.clone(),
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
        // A bound agent's thinking budget travels with it, same as its tier.
        let effort = run
            .get::<Option<String>, _>("agent_effort")
            .and_then(|e| ReasoningEffort::parse(&e));
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

        // Attachments are folded in here rather than at the route, so that
        // re-running a task re-attaches and `tasks.prompt` keeps holding
        // exactly what the user typed.
        let atts = attachments::for_task(&self.db, task_id).await.unwrap_or_default();
        let (prompt, extra_read_dirs) =
            attachments::augment_prompt(&run.get::<String, _>("prompt"), &atts);

        // A bound agent carries its memory into the run: what it did on this
        // project before is context, and what it does now becomes memory below.
        let bound_agent: Option<Uuid> = run.get("agent_id");
        let project_id: Uuid = run.get("project_id");
        let memory_block = match bound_agent {
            Some(agent_id) => memory::recall(&self.db, agent_id, Some(project_id))
                .await
                .ok()
                .as_deref()
                .and_then(memory::render),
            None => None,
        };

        let spec = RunSpec {
            cwd,
            prompt,
            model_tier: tier,
            model_id,
            effort,
            resume_session_id: run.get("session_id"),
            permission_mode,
            allowed_tools: agent_tools.unwrap_or_default(),
            append_system_prompt: match (agent_prompt.filter(|p| !p.is_empty()), memory_block) {
                (Some(p), Some(m)) => Some(format!("{p}{m}")),
                (Some(p), None) => Some(p),
                // Memory is useful even when the agent has no system prompt.
                (None, Some(m)) => Some(m),
                (None, None) => None,
            },
            mcp_config_path,
            extra_read_dirs,
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
            // Review exists to gate a diff onto the base branch. An in-place
            // run already wrote to the user's folder and produced no diff, so
            // parking it in review would offer a review that cannot happen.
            let column = if in_place { "done" } else { "review" };
            sqlx::query("UPDATE tasks SET board_column=$2 WHERE id=$1")
                .bind(task_id)
                .bind(column)
                .execute(&self.db.pool)
                .await?;

            // The work joins the agent's memory. Best-effort: a failed memory
            // write must not fail a completed run.
            if let Some(agent_id) = bound_agent {
                let title: String = run.get("title");
                let note = format!(
                    "Completed task \"{title}\": {}",
                    if outcome.output.is_empty() { "(no summary)" } else { &outcome.output }
                );
                if let Err(e) =
                    memory::remember(&self.db, agent_id, Some(project_id), Some(task_id), "task_result", &note).await
                {
                    tracing::warn!(%run_id, error = %e, "agent memory write failed");
                }
            }
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
            // Lateral join rather than a scalar subquery: the turn needs the
            // message's id as well as its text, to look up its attachments.
            "SELECT r.engine, c.session_id, c.session_engine, p.path AS project_path,
                    m.id AS user_message_id, m.content AS user_message
             FROM runs r JOIN chats c ON c.id = r.chat_id
             JOIN projects p ON p.id = c.project_id
             LEFT JOIN LATERAL (
                 SELECT id, content FROM chat_messages
                 WHERE chat_id = c.id AND role = 'user'
                 ORDER BY created_at DESC LIMIT 1
             ) m ON TRUE
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

        // An attachment-only turn stores empty content, so the prompt may be
        // nothing but the attachment block.
        let atts = match row.get::<Option<Uuid>, _>("user_message_id") {
            Some(message_id) => attachments::for_message(&self.db, message_id)
                .await
                .unwrap_or_default(),
            None => vec![],
        };
        let (user_message, extra_read_dirs) = attachments::augment_prompt(&user_message, &atts);

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
            effort: None,
            resume_session_id: session_id,
            permission_mode: PermissionMode::Reviewed,
            allowed_tools: CHAT_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect(),
            append_system_prompt: Some(CHAT_SYSTEM_PROMPT.to_string()),
            mcp_config_path,
            extra_read_dirs,
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

    /// An agent answering an @-mention on a task card.
    ///
    /// Structurally a chat-shaped run: cwd is the real checkout with read-only
    /// tools, so the agent can ground its answer in the code but can never
    /// edit anything from a comment. Thread context and the agent's memories
    /// travel in the prompt — replies are one-shot, not resumed sessions,
    /// because the thread itself is the durable state.
    async fn execute_comment_run(
        self: &Arc<Self>,
        run_id: Uuid,
        comment_id: Uuid,
    ) -> anyhow::Result<()> {
        let row = sqlx::query(
            "SELECT r.engine, r.agent_id, c.task_id, c.content AS mention,
                    t.title, t.prompt AS task_prompt, t.board_column,
                    p.id AS project_id, p.path AS project_path,
                    a.name AS agent_name, a.system_prompt, a.model_tier AS agent_tier,
                    a.effort AS agent_effort
             FROM runs r
             JOIN task_comments c ON c.id = r.comment_id
             JOIN tasks t ON t.id = c.task_id
             JOIN projects p ON p.id = t.project_id
             JOIN agents a ON a.id = r.agent_id
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

        let agent_id: Uuid = row.get("agent_id");
        let task_id: Uuid = row.get("task_id");
        let project_id: Uuid = row.get("project_id");
        let agent_name: String = row.get("agent_name");
        let title: String = row.get("title");

        // The thread so far, oldest first, excluding the mention itself —
        // that goes last, as the thing being answered.
        let thread: Vec<(String, Option<String>, String)> = sqlx::query_as(
            "SELECT c.author, a.name, c.content FROM task_comments c
             LEFT JOIN agents a ON a.id = c.agent_id
             WHERE c.task_id = $1 AND c.id <> $2
             ORDER BY c.created_at DESC LIMIT 15",
        )
        .bind(task_id)
        .bind(comment_id)
        .fetch_all(&self.db.pool)
        .await?;

        let memories = memory::recall(&self.db, agent_id, Some(project_id))
            .await
            .unwrap_or_default();

        let mut prompt = format!(
            "You were mentioned in a comment on the task card \"{title}\" \
             (column: {}).\n\nThe task brief:\n{}\n",
            row.get::<String, _>("board_column"),
            row.get::<String, _>("task_prompt"),
        );
        if !thread.is_empty() {
            prompt.push_str("\nThe discussion so far, oldest first:\n");
            for (author, name, content) in thread.iter().rev() {
                let who = match author.as_str() {
                    "agent" => name.clone().unwrap_or_else(|| "agent".into()),
                    _ => "user".into(),
                };
                prompt.push_str(&format!("[{who}] {content}\n"));
            }
        }
        if let Some(block) = memory::render(&memories) {
            prompt.push_str(&block);
        }
        prompt.push_str(&format!(
            "\nThe comment mentioning you:\n{}\n\n\
             Reply as a comment on this card: concise, concrete, grounded in this \
             repository (use Read/Grep/Glob to check before claiming). You cannot \
             edit files from here — if work is needed, describe it so the user can \
             run the task. Your entire output is posted verbatim as your comment: \
             no preamble, and never remark on tools, task lists, or how the \
             comment gets delivered.",
            row.get::<String, _>("mention"),
        ));

        let tier: ModelTier = serde_json::from_value(serde_json::Value::String(
            row.get::<String, _>("agent_tier"),
        ))
        .unwrap_or_default();
        let model_id = self.tiers.model_for(tier).to_string();
        sqlx::query("UPDATE runs SET model=$1, started_at=now() WHERE id=$2")
            .bind(&model_id)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;

        let system_prompt: String = row.get("system_prompt");
        let reply_effort = row
            .get::<Option<String>, _>("agent_effort")
            .and_then(|e| ReasoningEffort::parse(&e));
        let persona = format!(
            "You are {agent_name}, an agent on this project's kanban board.{}",
            if system_prompt.is_empty() {
                String::new()
            } else {
                format!("\n\n{system_prompt}")
            }
        );

        let spec = RunSpec {
            cwd: PathBuf::from(row.get::<String, _>("project_path")),
            prompt,
            model_tier: tier,
            model_id,
            effort: reply_effort,
            resume_session_id: None,
            permission_mode: PermissionMode::Reviewed,
            // Read-only, and no MCP: a comment reply answers, it doesn't act.
            allowed_tools: vec!["Read".into(), "Grep".into(), "Glob".into()],
            append_system_prompt: Some(persona),
            mcp_config_path: None,
            extra_read_dirs: vec![],
            permission_prompt_tool: false,
            extra_env: HashMap::from([("AICHIP_RUN_ID".to_string(), run_id.to_string())]),
        };

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let outcome = self.stream_run(run_id, None, &seq, engine, spec, true).await?;

        if outcome.status == RunStatus::Completed {
            let reply = if outcome.output.trim().is_empty() {
                "(no reply)".to_string()
            } else {
                outcome.output.clone()
            };
            sqlx::query(
                "INSERT INTO task_comments (task_id, author, agent_id, content, run_id)
                 VALUES ($1, 'agent', $2, $3, $4)",
            )
            .bind(task_id)
            .bind(agent_id)
            .bind(&reply)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
            // The exchange becomes memory, so the agent's next run knows it
            // happened.
            let note = format!(
                "On card \"{title}\": asked \"{}\" — I replied: {}",
                row.get::<String, _>("mention"),
                reply
            );
            memory::remember(
                &self.db,
                agent_id,
                Some(project_id),
                Some(task_id),
                "comment_reply",
                &note,
            )
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
            "SELECT w.source_yaml, w.name, r.engine, r.task_id, p.id AS project_id,
                    p.workspace_id, p.path AS project_path, p.default_branch,
                    p.full_auto_opt_in
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
        let task_id: Option<Uuid> = row.get("task_id");
        let shared = self
            .worktree_for_run(
                run_id,
                task_id,
                &project_path,
                &default_branch,
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
                            effort: agent.as_ref().and_then(|a| a.effort),
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
                            extra_read_dirs: vec![],
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

        let status = match failure {
            None => {
                self.finish(run_id, RunStatus::Completed, None).await?;
                sqlx::query("UPDATE workflows SET last_run_at = now() WHERE id = $1")
                    .bind(workflow_id)
                    .execute(&self.db.pool)
                    .await?;
                RunStatus::Completed
            }
            Some(reason) => {
                self.finish(run_id, RunStatus::Failed, Some(reason)).await?;
                RunStatus::Failed
            }
        };
        // A pipeline launched from a board task lands on review like any
        // other task run.
        self.settle_task_for_run(task_id, status).await?;
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

    pub(crate) async fn load_agent(
        &self,
        workspace_id: Uuid,
        name: Option<&str>,
    ) -> anyhow::Result<Option<BoundAgent>> {
        let Some(name) = name else { return Ok(None) };
        let row = sqlx::query(
            "SELECT system_prompt, model_tier, effort, allowed_tools, permission_preset
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
            effort: r
                .get::<Option<String>, _>("effort")
                .and_then(|e| ReasoningEffort::parse(&e)),
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
    pub(crate) async fn stream_run(
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

    pub(crate) async fn set_status(&self, run_id: Uuid, status: RunStatus) -> anyhow::Result<()> {
        sqlx::query("UPDATE runs SET status=$1 WHERE id=$2")
            .bind(status.as_str())
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn finish(
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

pub(crate) async fn next_seq(db: &Db, run_id: Uuid) -> anyhow::Result<i64> {
    let row = sqlx::query("SELECT COALESCE(MAX(seq), -1) + 1 AS next FROM events WHERE run_id=$1")
        .bind(run_id)
        .fetch_one(&db.pool)
        .await?;
    Ok(row.get("next"))
}

/// Write a per-run MCP config pointing the engine at one of aichip's MCP
/// endpoints. Lives under ~/.aichip/mcp/, keyed by run id.
pub(crate) async fn write_mcp_config(base_url: &str, url_path: &str, run_id: Uuid) -> anyhow::Result<PathBuf> {
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

pub(crate) fn slugify(s: &str) -> String {
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
