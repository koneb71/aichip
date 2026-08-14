//! Run orchestrator: owns the run state machine, the concurrency semaphore,
//! and the queue loop. All status transitions happen here and every event is
//! persisted before it is broadcast, so the DB is the source of truth.
//!
//! Two kinds of runs share the same machinery:
//! - task runs (board cards): isolated worktree, permission proxy
//! - chat runs (project assistant turns): real checkout cwd, but locked to a
//!   read-only + aichip-tools allowed list — never Bash/Edit/Write there.

use aichip_shared::workflow::{SessionMode, StepOutputs, Workflow};
use aichip_shared::{EngineTierEffort, EngineTierMapping, McpWiring, 
    AichipEvent, EventEnvelope, ModelTier, PermissionMode, ReasoningEffort, RunStatus,
    TierChoice,
};
use aichip_engines::{Engine, RunSpec};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use sqlx::Row;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::Ordering;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::apps;
use crate::bus::EventBus;
use crate::db::Db;
use crate::runs::attachments;
use crate::runs::memory;
use crate::runs::mentions;
use crate::runs::task_plan;
use crate::runs::usage_tally::{UsageDelta, UsageTally};
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
    "mcp__aichip__cancel_task",
    "mcp__aichip__get_diff",
    "mcp__aichip__get_spend",
    "mcp__aichip__list_skills",
    "mcp__aichip__move_task",
    "mcp__aichip__search_code",
];

/// Named explicitly rather than left to the allow-list.
///
/// `--allowedTools` pre-*approves*; it does not forbid. Anything the chat
/// assistant must never reach has to be denied by name or the CLI will happily
/// pick it up — and the assistant runs in the user's real checkout, not a
/// worktree, so an edit here would land straight on their files.
pub const CHAT_DENIED_TOOLS: &[&str] = &[
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "Bash",
];

/// The weakest permission mode this engine can actually honour for a read-only
/// tool set.
///
/// The chat assistant cannot edit anything: its whole surface is Read/Grep/Glob
/// plus aichip's own task tools, with the mutating tools denied above. So there
/// is nothing here for a human to review, and asking for review is not caution
/// — on an engine that cannot pause and ask, it is silence.
///
/// This used to be a flat `Reviewed`, which is why chat looked broken on
/// OpenCode: with no way to answer a prompt mid-run it rejects every tool call,
/// so the assistant could not read the repository or reach its own tools and
/// fell back to asking the user what they were working on. `vet` exists to
/// refuse exactly that pairing, and this was the one caller that never ran it.
fn chat_permission_mode(engine: &dyn aichip_engines::Engine) -> PermissionMode {
    if engine.capabilities().interactive_permissions {
        // Claude: the allow-list has already pre-approved the read tools, so
        // nothing prompts. AutoEdit rather than FullAuto keeps the dangerous
        // flag off a run that has no business editing anything.
        PermissionMode::AutoEdit
    } else {
        // OpenCode: approve-everything is the only setting it has, and
        // "everything" here is bounded by the denials above.
        PermissionMode::FullAuto
    }
}

const CHAT_SYSTEM_PROMPT: &str = "You are the aichip project assistant embedded in a workspace \
dashboard. You can inspect this repository (Read/Grep/Glob) but you cannot edit it directly. \
To do coding work, create a task with mcp__aichip__create_task (set start=true to launch it \
immediately); each task runs a coding agent in an isolated git worktree and its result appears \
on the user's board for review. Use mcp__aichip__list_tasks / get_task_status to report \
progress, and mcp__aichip__list_agents to pick a specialized agent for a task. When the user \
writes @Name in their message they are naming an agent from their library — assign that work to \
them by passing agent_name, spelled exactly. Keep replies short and conversational; the user \
sees them in a chat panel. \
You can also stop a task, summarise what it changed, file it in a column, and say what it has \
cost — but you cannot merge anything into the user's checkout, and there is no tool for it. \
When a card looks ready, say so and what it changed, and let them press Merge on the card, \
where they can read the diff first.";

/// The system prompt for a *general* chat: no project, no board, no repo.
///
/// Its own constant rather than the project prompt with caveats — a prompt
/// that describes ten tools the assistant does not have teaches it to promise
/// things every reply will then fail to deliver.
const GENERAL_CHAT_SYSTEM_PROMPT: &str = "You are the aichip assistant. This conversation is \
not attached to any project: there is no repository to inspect, no board to create tasks on, \
and no code you can see. You can search the web (WebSearch) and read pages (WebFetch) to \
answer questions, and you can reason and write. If the user asks for coding work on one of \
their projects, tell them to switch this chat to that project — the picker is above the \
conversation list. Keep replies short and conversational; the user sees them in a chat panel.";

/// Tools for a chat scoped to a *space* — a folder of documents, not a repo.
///
/// The read tools point at the documents (which is the whole point of a
/// space: drop files in, ask about them), the web tools cover what the
/// documents reference. No board tools — see the MCP wiring decision — and
/// the same denials as every chat, because the run stands in the user's real
/// folder.
pub const SPACE_CHAT_ALLOWED_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "WebSearch",
    "WebFetch",
    "mcp__aichip__search_documents",
    "mcp__aichip__list_documents",
];

/// The system prompt for a space chat.
const SPACE_CHAT_SYSTEM_PROMPT: &str = "You are the aichip assistant. This conversation is attached to a document space: the folder you are in holds documents the user has collected, and your job is to answer questions about and around them. Relevant passages from the documents are attached to each message automatically, with file names — cite them when you draw on them. Use mcp__aichip__search_documents to search again with a different phrasing, mcp__aichip__list_documents to see what is here, and Read to open a whole file; WebSearch and WebFetch cover what the documents reference. Ground answers about the documents in what they actually say — quote or name the file — and say plainly when the answer is not in them. There is no repository here and no task board; if the user asks for coding work, tell them to switch this chat to one of their projects. Keep replies short and conversational; the user sees them in a chat panel.";

/// One attempt in a bake-off: an agent, a tier, or both.
///
/// Either field may be absent — "the same agent at three effort levels" and
/// "three different agents at their own tiers" are both things you'd want to
/// compare, and the label is what the user reads either way.
#[derive(Debug, Clone)]
pub struct Variant {
    pub label: String,
    pub agent_id: Option<Uuid>,
    pub tier: Option<String>,
    /// Which CLI runs this attempt. `None` means the card's. This is what
    /// makes "Claude vs OpenCode on the same brief" a thing you can ask for.
    pub engine: Option<String>,
}

/// Why the queue is, or isn't, dispatching.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueueGate {
    Open,
    /// Someone pressed pause. Cleared by pressing resume.
    Paused,
    /// Today's spend reached the cap. Clears itself at midnight — there is no
    /// resume for this one, which is why it can't just be a bool.
    OverBudget { spent_today: f64, cap_usd: f64 },
}

pub struct Orchestrator {
    pub db: Db,
    pub bus: EventBus,
    engines: HashMap<&'static str, Arc<dyn Engine>>,
    /// What `detect()` said at boot, kept so the engines endpoint can answer
    /// without spawning three CLIs on every page load.
    detected: HashMap<&'static str, aichip_engines::EngineInfo>,
    /// Tier → model routing, live rather than baked at boot: changing it in
    /// settings must affect the next run, not require a restart.
    tiers: Arc<std::sync::RwLock<EngineTierMapping>>,
    tier_efforts: Arc<std::sync::RwLock<EngineTierEffort>>,
    pub worktrees: Arc<WorktreeManager>,
    pub(crate) slots: Arc<crate::runs::slots::Slots>,
    /// Whether the over-budget hook has already fired for the current spell.
    /// The gate is read every 750ms, so this is what turns a state into an
    /// event.
    announced_over_budget: std::sync::atomic::AtomicBool,
    /// Base URL of aichip's MCP endpoints, e.g. "http://127.0.0.1:4820".
    /// None disables MCP wiring (mock engine / tests).
    pub(crate) mcp_base_url: Option<String>,
    /// Keyed by run, never by step — see `CancelState`.
    cancels: Mutex<HashMap<Uuid, CancelState>>,
}

pub(crate) struct StreamOutcome {
    pub status: RunStatus,
    pub reason: Option<String>,
    /// Final assistant/result text, used to feed later pipeline steps.
    pub output: String,
    pub session_id: Option<String>,
}

/// Cancellation for a run that may be several steps long.
///
/// `requested` is the part that matters: interrupting the process running
/// right now is not enough, because a workflow or organization would simply
/// start the next assignment. The flag is checked between steps, so asking
/// to cancel stops the whole run rather than one step of it.
#[derive(Default)]
pub(crate) struct CancelState {
    requested: bool,
    /// Live steps to interrupt. Several at once during a fan-out.
    steps: HashMap<Uuid, oneshot::Sender<()>>,
}

/// An agent from the library, resolved for a step or task.
pub(crate) struct BoundAgent {
    pub id: Uuid,
    pub system_prompt: String,
    pub tier: ModelTier,
    pub effort: Option<ReasoningEffort>,
    pub allowed_tools: Vec<String>,
    /// `None` means "inherit" — nullable since migration 0020. Decoding this
    /// as a plain `String` panics on every agent that inherits, which is now
    /// all of them by default.
    pub permission_preset: Option<String>,
    /// `None` means "whatever the workflow or card says".
    pub engine: Option<String>,
}

/// A step's permission mode, and whether it got there by being cut down.
///
/// The distinction matters because the two produce the same mode for very
/// different reasons, and only one of them is something the author wrote.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StepPermission {
    pub mode: PermissionMode,
    /// True when FullAuto was refused and Reviewed substituted.
    pub downgraded: bool,
}

/// Apply the FullAuto safety gate.
///
/// Pure so it can be tested without a database or a worktree on disk — the
/// gate is a safety property and deserves to be pinned down directly rather
/// than inferred from an integration run.
pub(crate) fn resolve_step_permission(asked: PermissionMode, gate_satisfied: bool) -> StepPermission {
    if asked == PermissionMode::FullAuto && !gate_satisfied {
        // Down, never up: refusing FullAuto is de-escalation and safe.
        StepPermission { mode: PermissionMode::Reviewed, downgraded: true }
    } else {
        StepPermission { mode: asked, downgraded: false }
    }
}

/// Which of the six things that stream an engine this dispatch is.
///
/// It replaced a bare `finalize: bool`, because the two questions it answers
/// were being answered independently and one of them was never asked at all.
/// A caller has to say what it *is*, and the answers follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallerKind {
    /// A card doing its work.
    TaskWork,
    /// A card writing the plan it will be asked to approve.
    TaskPlanning,
    /// One step of a workflow.
    WorkflowStep,
    /// One teammate's turn inside an organization run.
    OrgMember,
    /// A chat turn in the assistant panel.
    Chat,
    /// A one-shot reply to a comment.
    CommentReply,
    /// Generating a knowledge-base article.
    KbGeneration,
    /// Investigating a question about a project: read-only plus the web,
    /// producing a report.
    Research,
}

/// What to do when the engine reports a rate limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnRateLimit {
    /// Mark the run held and put it back on the queue behind a backoff.
    Hold,
    /// Give up and let the caller finish the run. Re-dispatching would charge
    /// again for work that is already paid for.
    Fail,
}

impl CallerKind {
    /// Does `stream_run` own this run's ending?
    ///
    /// False where a *step* ending is not a *run* ending: a plan that
    /// completed is not a completed run, and marking it terminal would send
    /// the card to review with nothing done. Workflows and organizations
    /// decide their own outcome once every step is in.
    pub(crate) fn finalizes(self) -> bool {
        !matches!(self, Self::TaskPlanning | Self::WorkflowStep | Self::OrgMember)
    }

    /// Can this run be handed back to `execute` later, unchanged?
    ///
    /// This is the whole question, and it was never asked: `stream_run` wrote
    /// a queue row for *every* caller, so a workflow step that hit a limit put
    /// its run back on the queue, the workflow then failed the run, `finish`
    /// left the row behind, and five minutes later `claim_next` popped a run
    /// marked `failed` and re-ran the entire pipeline — paying again for every
    /// step that had already succeeded.
    ///
    /// Exhaustive on purpose. A seventh caller is a compile error here rather
    /// than a silent leak, which is how the first six got one.
    pub(crate) fn on_rate_limit(self) -> OnRateLimit {
        match self {
            // `execute_task_run` reuses the card's worktree and rebuilds its
            // spec from the row, so a second dispatch continues rather than
            // duplicates. Everything after `stream_run` is guarded on
            // `Completed`, so a held run falls straight through it.
            Self::TaskWork => OnRateLimit::Hold,
            // Same, plus one early return: `park_for_approval` treats "not
            // completed" as "no plan to approve" and fails the run, which
            // would undo the hold a line later.
            Self::TaskPlanning => OnRateLimit::Hold,
            // Nothing is written until the article completes, and the run row
            // still carries the brief.
            Self::KbGeneration => OnRateLimit::Hold,
            // Same shape: the report is written only on completion, and the
            // run row carries `research_id`, so a re-dispatch rebuilds the
            // identical spec.
            Self::Research => OnRateLimit::Hold,
            // Both post exactly once, on completion, so a second dispatch
            // produces one reply rather than two.
            Self::Chat | Self::CommentReply => OnRateLimit::Hold,
            // `create_step_row` and `outputs` are written per dispatch, so a
            // re-run of a half-finished pipeline is charged for in full and
            // duplicates its own step rows. Failing honestly beats that; a
            // workflow that resumes from its `steps` rows is its own feature.
            Self::WorkflowStep => OnRateLimit::Fail,
            // The one that reads like it should hold and cannot. A member is
            // a *step* of an org run: the batch loop is still driving other
            // assignments in parallel and will finish the run itself, so a
            // hold written here is overwritten by a terminal status moments
            // later — leaving exactly the orphan queue row this is meant to
            // stop. Stopping a batch cleanly mid-flight is a feature, not a
            // guard, so the assignment fails and says why.
            Self::OrgMember => OnRateLimit::Fail,
        }
    }
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
            detected: HashMap::new(),
            tiers: Arc::new(std::sync::RwLock::new(EngineTierMapping::default())),
            tier_efforts: Arc::new(std::sync::RwLock::new(EngineTierEffort::default())),
            worktrees,
            slots: Arc::new(crate::runs::slots::Slots::new(max_concurrent)),
            announced_over_budget: std::sync::atomic::AtomicBool::new(false),
            mcp_base_url,
            cancels: Mutex::new(HashMap::new()),
        }
    }

    /// What to set `MCP_TOOL_TIMEOUT` to, in milliseconds.
    ///
    /// Derived from the attention window rather than written out, because the
    /// two numbers have to agree and used to agree only by comment. The CLI
    /// abandoning the tool call before the broker's window closes is the same
    /// silent lie the window was widened to remove — the engine would get a
    /// timeout error nobody chose, at a moment when the person was still
    /// perfectly able to answer.
    ///
    /// "Wait forever" has no expressible value here, so it becomes the ceiling
    /// the setting itself clamps to.
    ///
    /// The margin is load-bearing, not padding. Set the two to the same number
    /// and they race — and the CLI won, so the engine got
    /// `MCP server "aichip" tool "approve" timed out after 60s` instead of
    /// aichip's own sentence, then carried on trying other ways to do the same
    /// edit. The broker has to be the one that decides how a wait ends,
    /// because it is the only one of the two that knows the difference between
    /// a refusal and an empty room.
    pub(crate) async fn mcp_tool_timeout_ms(&self) -> String {
        crate::attention::cli_timeout_ms(crate::attention::load(&self.db).await.window())
    }

    /// The queue's concurrency budget, so the permission broker can lend a
    /// parked run's slot back while it waits.
    pub fn slots(&self) -> Arc<crate::runs::slots::Slots> {
        self.slots.clone()
    }

    pub fn register_engine(&mut self, engine: Arc<dyn Engine>) {
        self.engines.insert(engine.id(), engine);
    }

    /// Register an engine only if its CLI is actually installed.
    ///
    /// An engine that isn't here is simply *not offered* — which is a far
    /// better failure than accepting the choice and then dying at spawn time,
    /// minutes later and nowhere near where the user made it.
    ///
    /// Returns what `detect()` found, so the caller can log it.
    pub async fn register_if_available(
        &mut self,
        engine: Arc<dyn Engine>,
    ) -> Option<aichip_engines::EngineInfo> {
        let info = engine.detect().await?;
        self.detected.insert(engine.id(), info.clone());
        self.engines.insert(engine.id(), engine);
        Some(info)
    }

    /// What `detect()` reported at boot. `None` for engines registered
    /// without a probe (the mock).
    pub fn engine_info(&self, id: &str) -> Option<&aichip_engines::EngineInfo> {
        self.detected.get(id)
    }

    pub fn engine(&self, id: &str) -> Option<Arc<dyn Engine>> {
        self.engines.get(id).cloned()
    }

    /// Every engine that was actually found at boot. Sorted by id so the UI
    /// column order doesn't shuffle between restarts (a `HashMap` iteration
    /// order would).
    pub fn engines(&self) -> Vec<Arc<dyn Engine>> {
        let mut v: Vec<_> = self.engines.values().cloned().collect();
        v.sort_by_key(|e| e.id());
        v
    }

    /// What to run when nothing upstream stated a preference.
    ///
    /// Claude Code when it's installed — it's the one engine that can do
    /// everything aichip offers, including stopping to ask permission. Only
    /// when it's absent does something else become the default, and then the
    /// alternative to picking one is offering the user nothing at all.
    pub fn default_engine(&self) -> String {
        if self.engines.contains_key("claude-code") {
            return "claude-code".into();
        }
        self.engines()
            .into_iter()
            .find(|e| e.id() != "mock")
            .map(|e| e.id().to_string())
            .unwrap_or_else(|| "claude-code".into())
    }

    /// Create a run for a board task and put it on the queue. A task handed
    /// to a team runs as that team instead of a single agent.
    pub async fn enqueue_task(&self, task_id: Uuid) -> anyhow::Result<Uuid> {
        // The last line of defence against two agents in one checkout.
        //
        // A sub-ticket's work is already running under a step of its epic's run,
        // and starting it again would put a second agent in the same worktree
        // with a second writer for the card's column. The routes refuse this
        // with a 409, but they are not the only door: dropping a card into
        // "In Progress", Retry, and the chat MCP's `start_task` all arrive here
        // directly.
        let live: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM steps s JOIN runs r ON r.id = s.run_id
                  WHERE s.task_id = $1
                    AND s.status IN ('queued','starting','running','waiting_permission','rate_limited')
                    AND r.status NOT IN ('completed','failed','canceled'))",
        )
        .bind(task_id)
        .fetch_one(&self.db.pool)
        .await?;
        if live {
            anyhow::bail!("a teammate is already working on this sub-task as part of its epic");
        }

        // Dependencies hold until the blocker's work has LANDED — 'done', not
        // 'review'. A blocker in review has a diff nobody merged; a dependent
        // run started then would branch from main without the work it builds
        // on. Checked here, the one door every start comes through (routes,
        // drag, retry, the chat MCP's start_task), not only in the UI.
        let blockers: Vec<String> = sqlx::query_scalar(
            "SELECT b.title FROM task_deps d JOIN tasks b ON b.id = d.blocked_by
             WHERE d.task_id = $1 AND b.board_column <> 'done' ORDER BY b.title",
        )
        .bind(task_id)
        .fetch_all(&self.db.pool)
        .await?;
        if !blockers.is_empty() {
            anyhow::bail!(
                "blocked by {} — land {} first",
                blockers.join(", "),
                if blockers.len() == 1 { "that card" } else { "those cards" }
            );
        }

        let assigned_team: Option<Uuid> = sqlx::query("SELECT team_id FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&self.db.pool)
            .await?
            .get("team_id");
        if let Some(team_id) = assigned_team {
            return self.enqueue_task_for_team(task_id, team_id).await;
        }

        // The bound agent's engine wins over the card's: an agent that names
        // one has been deliberately configured for it, while a card's is the
        // machine default nobody chose.
        let row = sqlx::query(
            "INSERT INTO runs (task_id, status, trigger, engine, plan_approval)
             SELECT t.id, 'queued', 'manual', COALESCE(a.engine, t.engine), t.plan_first
             FROM tasks t LEFT JOIN agents a ON a.id = t.agent_id
             WHERE t.id = $1
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

    /// Pick a dead run back up: a new run row carrying the old one's session.
    ///
    /// Everything that decides *whether* this is allowed lives in
    /// `runs::resume` and has already run by the time this is called — the
    /// route answers the person, this writes the row. See that module for why
    /// resuming creates a row rather than re-queuing the old one.
    pub async fn resume_run(&self, prior_run_id: Uuid, session_id: &str) -> anyhow::Result<Uuid> {
        let prior = sqlx::query(
            "SELECT r.task_id, r.engine, r.agent_id, r.tier_override, r.variant_label,
                    r.worktree_path, r.error_reason,
                    COALESCE(r.prompt_override, t.prompt) AS prompt
             FROM runs r JOIN tasks t ON t.id = r.task_id
             WHERE r.id = $1",
        )
        .bind(prior_run_id)
        .fetch_one(&self.db.pool)
        .await?;

        let prompt = crate::runs::resume::continuation_prompt(
            &prior.get::<String, _>("prompt"),
            prior.get::<Option<String>, _>("error_reason").as_deref(),
        );

        let row = sqlx::query(
            // `plan_approval` is deliberately FALSE and not copied. A
            // plan-first run whose plan was never stored plans again — see
            // `a_plan_first_run_with_no_stored_plan_plans_again` — so carrying
            // it forward would hand a session that has been writing code a
            // planning pass with the mutating tools denied, and it would
            // produce a second plan instead of finishing the work.
            //
            // `comment_id` is likewise omitted rather than copied: with it set
            // `execute` routes to `execute_comment_run`, which builds its own
            // prompt and posts another reply. Only a card run gets here at all
            // (`Refusal::NotATaskRun`), so there is nothing to carry.
            "INSERT INTO runs (task_id, status, trigger, engine, plan_approval,
                               agent_id, tier_override, variant_label, worktree_path,
                               session_id, session_engine, prompt_override, resumed_from)
             VALUES ($1, 'queued', 'resume', $2, FALSE, $3, $4, $5, $6, $7, $2, $8, $9)
             RETURNING id",
        )
        .bind(prior.get::<Uuid, _>("task_id"))
        .bind(prior.get::<String, _>("engine"))
        .bind(prior.get::<Option<Uuid>, _>("agent_id"))
        .bind(prior.get::<Option<String>, _>("tier_override"))
        .bind(prior.get::<Option<String>, _>("variant_label"))
        .bind(prior.get::<Option<String>, _>("worktree_path"))
        .bind(session_id)
        .bind(&prompt)
        .bind(prior_run_id)
        .fetch_one(&self.db.pool)
        .await?;
        let run_id: Uuid = row.get("id");
        // Same priority a manual start gets: this *is* a manual start, of work
        // that is further along than a fresh one.
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

    /// One brief, several attempts, each in its own checkout.
    ///
    /// Deliberately does *not* touch the task's own worktree: until a winner
    /// is picked the card is unchanged, so a bake-off you abandon costs
    /// nothing but the tokens.
    pub async fn enqueue_bakeoff(
        &self,
        task_id: Uuid,
        variants: &[Variant],
    ) -> anyhow::Result<Vec<Uuid>> {
        if variants.len() < 2 {
            anyhow::bail!("a bake-off needs at least two variants to compare");
        }
        let engine: String = sqlx::query("SELECT engine FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&self.db.pool)
            .await?
            .get("engine");

        let mut ids = vec![];
        for variant in variants {
            let row = sqlx::query(
                "INSERT INTO runs (task_id, agent_id, tier_override, variant_label,
                                   status, trigger, engine)
                 VALUES ($1,$2,$3,$4,'queued','bakeoff',$5) RETURNING id",
            )
            .bind(task_id)
            .bind(variant.agent_id)
            .bind(variant.tier.as_deref())
            .bind(&variant.label)
            .bind(variant.engine.as_ref().unwrap_or(&engine))
            .fetch_one(&self.db.pool)
            .await?;
            let run_id: Uuid = row.get("id");
            // Below interactive work: a bake-off is exploratory, and it is
            // several runs at once against one rate limit.
            sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, 8)")
                .bind(run_id)
                .execute(&self.db.pool)
                .await?;
            ids.push(run_id);
        }
        Ok(ids)
    }

    /// Adopt one variant's work as the task's, and throw the rest away.
    ///
    /// The winner's worktree *becomes* the card's, rather than being copied
    /// or merged: it already holds the branch, the commits, and the diff the
    /// user just read and chose.
    pub async fn keep_variant(&self, run_id: Uuid) -> anyhow::Result<()> {
        let row = sqlx::query(
            "SELECT r.task_id, r.worktree_path, r.variant_label, p.path AS project_path
             FROM runs r
             JOIN tasks t ON t.id = r.task_id
             JOIN projects p ON p.id = t.project_id
             WHERE r.id = $1 AND r.variant_label IS NOT NULL",
        )
        .bind(run_id)
        .fetch_optional(&self.db.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("that run is not a bake-off variant"))?;

        let task_id: Uuid = row.get("task_id");
        let project_path: String = row.get("project_path");
        let worktree: String = row
            .get::<Option<String>, _>("worktree_path")
            .ok_or_else(|| anyhow::anyhow!("that variant never produced a checkout"))?;

        let branch = crate::worktrees::manager::current_branch(std::path::Path::new(&worktree))
            .await
            .unwrap_or_default();

        sqlx::query(
            "UPDATE tasks SET worktree_path = $1, branch = $2, board_column = 'review'
             WHERE id = $3",
        )
        .bind(&worktree)
        .bind(&branch)
        .bind(task_id)
        .execute(&self.db.pool)
        .await?;

        // The losers' checkouts are removed, but their runs stay: what the
        // other attempts cost and produced is the record of why this one won.
        let losers: Vec<(Uuid, Option<String>)> = sqlx::query_as(
            "SELECT id, worktree_path FROM runs
             WHERE task_id = $1 AND variant_label IS NOT NULL AND id <> $2",
        )
        .bind(task_id)
        .bind(run_id)
        .fetch_all(&self.db.pool)
        .await?;
        for (loser, path) in losers {
            if let Some(path) = path {
                let repo = std::path::Path::new(&project_path);
                if let Err(e) = self.worktrees.discard(repo, std::path::Path::new(&path)).await {
                    // Disk left behind is untidy, not broken.
                    tracing::warn!(%loser, error=%e, "could not remove losing variant's worktree");
                }
            }
            // Forget the path whether or not the removal worked, and this is
            // load-bearing twice over. The row pointed at a directory that is
            // gone; and `worktrees::sweep` treats every path any row names as
            // *claimed*, so a stale one made that directory permanently
            // unsweepable — the sweep would skip it forever while nothing else
            // could ever name it either.
            let _ = sqlx::query("UPDATE runs SET worktree_path = NULL WHERE id = $1")
                .bind(loser)
                .execute(&self.db.pool)
                .await;
        }
        Ok(())
    }

    /// Act on a review note left against a line of the diff.
    ///
    /// A task run rather than a comment reply, and that distinction is the
    /// whole feature: comment replies are read-only by design, so the only
    /// way to act on review feedback was to re-run the entire task. This
    /// reuses the task's existing worktree, so the fix lands on the same
    /// branch and shows up in the same diff you were reading.
    pub async fn enqueue_review_fix(&self, comment_id: Uuid) -> anyhow::Result<Uuid> {
        let c = sqlx::query(
            "SELECT c.task_id, c.content, c.file_path, c.line, c.hunk, t.engine, t.prompt
             FROM task_comments c JOIN tasks t ON t.id = c.task_id WHERE c.id = $1",
        )
        .bind(comment_id)
        .fetch_one(&self.db.pool)
        .await?;

        let prompt = review_fix_prompt(
            &c.get::<String, _>("prompt"),
            c.get::<Option<String>, _>("file_path").as_deref(),
            c.get::<Option<i32>, _>("line"),
            c.get::<Option<String>, _>("hunk").as_deref(),
            &c.get::<String, _>("content"),
        );

        let row = sqlx::query(
            "INSERT INTO runs (task_id, comment_id, prompt_override, status, trigger, engine)
             VALUES ($1, $2, $3, 'queued', 'review', $4) RETURNING id",
        )
        .bind(c.get::<Uuid, _>("task_id"))
        .bind(comment_id)
        .bind(&prompt)
        .bind(c.get::<String, _>("engine"))
        .fetch_one(&self.db.pool)
        .await?;
        let run_id: Uuid = row.get("id");
        // Above a normal task run: someone is sitting there reading the diff.
        sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, 14)")
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
        // The workflow's own `defaults.engine` is authoritative — dispatch
        // reads it too. Recording it here means the activity list shows the
        // truth for the window between queued and running, rather than
        // whatever literal happened to be in this INSERT.
        let engine = sqlx::query_scalar::<_, String>(
            "SELECT source_yaml FROM workflows WHERE id = $1",
        )
        .bind(workflow_id)
        .fetch_optional(&self.db.pool)
        .await?
        .and_then(|yaml| Workflow::from_yaml(&yaml).ok())
        .map(|w| w.defaults.engine)
        .unwrap_or_else(|| self.default_engine());

        let row = sqlx::query(
            "INSERT INTO runs (workflow_id, status, trigger, engine)
             VALUES ($1, 'queued', $2, $3) RETURNING id",
        )
        .bind(workflow_id)
        .bind(trigger)
        .bind(&engine)
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

    /// Ask a run to stop. Returns true if it was executing — a queued or
    /// parked run has no process to interrupt and is handled by the caller.
    pub fn cancel(&self, run_id: Uuid) -> bool {
        let mut cancels = self.cancels.lock().unwrap();
        let state = cancels.entry(run_id).or_default();
        state.requested = true;
        let live = std::mem::take(&mut state.steps);
        let was_running = !live.is_empty();
        for (_, tx) in live {
            let _ = tx.send(());
        }
        was_running
    }

    /// Has someone asked this run to stop? Checked between steps.
    pub(crate) fn cancel_requested(&self, run_id: Uuid) -> bool {
        self.cancels
            .lock()
            .unwrap()
            .get(&run_id)
            .is_some_and(|c| c.requested)
    }

    /// Drop cancellation bookkeeping once a run reaches a terminal state, so
    /// the map doesn't grow for the life of the process.
    pub(crate) fn forget_cancel(&self, run_id: Uuid) {
        self.cancels.lock().unwrap().remove(&run_id);
    }

    /// On boot: anything left in starting/running died with the previous
    /// process. Mark failed; the UI offers one-click resume via session_id.
    pub async fn recover_orphans(&self) -> anyhow::Result<u64> {
        let mut tx = self.db.pool.begin().await?;
        // A run that was waiting on a person already recorded what it was
        // waiting for, and "orphaned by server restart" would throw that away.
        // This is the whole reason pending prompts need no table of their own:
        // the restart kills the run either way, and all persistence would have
        // bought is a truthful sentence, which `park` has already written.
        let orphans: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE runs SET status='failed',
                    error_reason = CASE WHEN status='waiting_permission'
                        THEN 'the server restarted while this run was '
                             || COALESCE(error_reason, 'waiting for you')
                        ELSE 'orphaned by server restart' END,
                    finished_at=now()
             WHERE status IN ('starting','running','waiting_permission')
             RETURNING id",
        )
        .fetch_all(&mut *tx)
        .await?;
        // The steps died with the run. Leaving them at 'running' is what makes
        // a failed team run still animate a teammate as "working…".
        settle_steps(&mut tx, &orphans, RunStatus::Failed).await?;
        // Nothing terminal keeps a place in the queue. This is the sweep for
        // rows written before `finish` learned to delete them — without it,
        // every failure this repository has already recorded stays claimable
        // and gets re-dispatched on the next boot.
        sqlx::query(
            "DELETE FROM queue q USING runs r
              WHERE r.id = q.run_id AND r.status IN ('completed','failed','canceled')",
        )
        .execute(&mut *tx)
        .await?;
        // And the mirror image: a run that says it is held has to actually be
        // waiting for something. A crash between `hold_rate_limited`'s two
        // statements, or a queue row deleted by hand, leaves `rate_limited`
        // with nothing to bring it back — a run that is neither running nor
        // finished and never will be. Behind the same backoff it would have
        // had, so a restart loop cannot become a retry storm.
        sqlx::query(
            "INSERT INTO queue (run_id, priority, not_before)
             SELECT r.id, 5, now() + interval '5 minutes' FROM runs r
              WHERE r.status = 'rate_limited'
                AND NOT EXISTS (SELECT 1 FROM queue q WHERE q.run_id = r.id)
             ON CONFLICT (run_id) DO NOTHING",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        // And so do the cards those steps stand for — otherwise a restart leaves
        // a column of sub-tickets stuck at "In Progress" with nothing behind
        // them, which is the same lie `settle_steps` exists to prevent.
        for run_id in &orphans {
            if let Err(e) = crate::runs::org::epic::mirror_run(&self.db, *run_id).await {
                tracing::warn!(%run_id, error=%e, "could not update this run's sub-task cards");
            }
        }
        Ok(orphans.len() as u64)
    }

    /// Queue loop: claim ready runs under the concurrency semaphore.
    pub async fn run_loop(self: Arc<Self>) {
        loop {
            let permit = self.slots.sem().acquire_owned().await.expect("semaphore");
            // A run that parked on a permission prompt lent this slot to the
            // queue and has since taken it back. Pay that debt here rather than
            // where the person clicked Allow: their engine is holding an HTTP
            // request open, and making the click wait on someone else's
            // twenty-minute run is how the call they just approved times out.
            if self.slots.take_debt() {
                permit.forget();
                continue;
            }
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

    /// Would this engine refuse this mode? Checked before a run is queued so
    /// the answer lands where the user is standing, not forty minutes later.
    ///
    /// Returns the reason, or `None` when the pairing is fine. An unknown
    /// engine is *not* an error here — dispatch reports that with a better
    /// message than "capability check failed".
    pub fn vet_engine(&self, engine_id: &str, mode: PermissionMode) -> Option<String> {
        let engine = self.engine(engine_id)?;
        aichip_engines::vet(engine.as_ref(), mode, false).err()
    }

    /// Which model this engine runs at this tier right now.
    ///
    /// Takes the engine because "medium" cannot mean one model globally —
    /// `claude-opus-5` is not something OpenCode can be asked for.
    pub fn model_for(&self, engine: &str, tier: ModelTier) -> String {
        self.tiers.read().unwrap().model_for(engine, tier)
    }

    /// A snapshot, for callers that need the whole mapping. Cloned rather
    /// than lent, so no lock guard is ever held across an await.
    pub fn tier_mapping(&self) -> EngineTierMapping {
        self.tiers.read().unwrap().clone()
    }

    /// Load the user's routing from settings. Called once at boot; anything
    /// missing or unparseable falls back to the built-in mapping rather than
    /// leaving runs with no model at all.
    pub async fn load_tier_mapping(&self) -> anyhow::Result<()> {
        let stored: Option<serde_json::Value> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'tier_models'")
                .fetch_optional(&self.db.pool)
                .await?;
        // Which engines the user has actually configured, read off the raw
        // JSON rather than the parsed struct: parsing fills in defaults for
        // every engine, which would make "they chose this" indistinguishable
        // from "we invented it".
        let explicit: std::collections::BTreeSet<String> = stored
            .as_ref()
            .and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        let mut mapping = stored
            .and_then(|v| serde_json::from_value::<EngineTierMapping>(v).ok())
            .unwrap_or_default();

        // For an engine nobody has configured, the built-in default is a
        // guess — and for a multi-provider engine it's usually a wrong one.
        // The install itself knows better, so ask it.
        for engine in self.engines() {
            if explicit.contains(engine.id()) {
                continue;
            }
            let Some(info) = self.detected.get(engine.id()) else {
                continue;
            };
            if let Some(picked) = aichip_shared::pick_defaults(&info.models) {
                tracing::info!(
                    engine = engine.id(),
                    medium = %picked.model_for(ModelTier::Medium),
                    "tier defaults derived from the models this install can reach"
                );
                mapping.0.insert(engine.id().to_string(), picked);
            }
        }
        *self.tiers.write().unwrap() = mapping;
        Ok(())
    }

    /// How hard each tier thinks, per engine. A snapshot, for the same reason
    /// `tier_mapping` returns one.
    pub fn tier_efforts(&self) -> EngineTierEffort {
        self.tier_efforts.read().unwrap().clone()
    }

    /// Unlike the model mapping there is nothing to invent when this is
    /// missing: no entry means inherit, which is a real answer.
    pub async fn load_tier_efforts(&self) -> anyhow::Result<()> {
        let stored: Option<serde_json::Value> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'tier_efforts'")
                .fetch_optional(&self.db.pool)
                .await?;
        let map = stored
            .and_then(|v| serde_json::from_value::<EngineTierEffort>(v).ok())
            .unwrap_or_default();
        *self.tier_efforts.write().unwrap() = map;
        Ok(())
    }

    pub async fn set_tier_efforts(&self, efforts: EngineTierEffort) -> anyhow::Result<()> {
        self.put_setting("tier_efforts", serde_json::to_value(&efforts)?)
            .await?;
        *self.tier_efforts.write().unwrap() = efforts;
        Ok(())
    }

    /// The budget a run should actually think with.
    ///
    /// Four places can have an opinion and the most specific wins: whatever was
    /// pinned on the bound agent or the card, then what this engine's tier
    /// says, then the machine default, then the CLI's own. Every step is
    /// resolved when the run starts rather than frozen at create time, so
    /// changing any of them reaches work already sitting in the backlog.
    ///
    /// One function rather than three copies, because the three call sites —
    /// a card, a chat turn, a teammate's reply — drifted apart the last time
    /// they were written out separately.
    pub async fn resolve_effort(
        &self,
        agent: Option<ReasoningEffort>,
        card: Option<ReasoningEffort>,
        engine: &str,
        tier: ModelTier,
    ) -> Option<ReasoningEffort> {
        // Bound out of the expression so no lock guard is alive across the
        // await below.
        let from_tier = self.tier_efforts.read().unwrap().effort_for(engine, tier);
        aichip_shared::resolve_effort(agent, card, from_tier, self.default_effort().await).0
    }

    /// Persist and apply a new routing.
    pub async fn set_tier_mapping(&self, mapping: EngineTierMapping) -> anyhow::Result<()> {
        for (engine, tiers) in &mapping.0 {
            for (tier, model) in &tiers.0 {
                if !aichip_shared::is_known_model_for(engine, model) {
                    anyhow::bail!("{model} is not a model {engine} can run (tier {tier:?})");
                }
            }
        }
        self.put_setting("tier_models", serde_json::to_value(&mapping)?)
            .await?;
        *self.tiers.write().unwrap() = mapping;
        Ok(())
    }

    /// Is the queue paused? Read per tick rather than cached, so the pause
    /// takes effect on the next claim instead of whenever a process happens
    /// to restart.
    pub async fn queue_paused(&self) -> bool {
        matches!(self.queue_gate().await, QueueGate::Paused)
    }

    /// Why the queue is or isn't handing out work.
    ///
    /// One answer for both reasons it can stop, because the UI has to tell
    /// them apart: a pause you chose is resumed with a click, while a spent
    /// budget clears on its own at midnight and a resume button would be a
    /// lie.
    pub async fn queue_gate(&self) -> QueueGate {
        // Scalar subqueries rather than aggregates: `value` is jsonb, and
        // there is no max(jsonb) — an aggregate here fails at runtime, which
        // the fallback below would quietly turn into "queue wide open".
        let settings = sqlx::query(
            "SELECT (SELECT value FROM settings WHERE key = 'queue_paused')     AS paused,
                    (SELECT value FROM settings WHERE key = 'daily_budget_usd') AS budget",
        )
        .fetch_optional(&self.db.pool)
        .await;

        let row = match settings {
            Ok(Some(row)) => row,
            // Never fail closed on a read error — a database hiccup must not
            // silently stop every run on the machine. But say so: failing
            // open without a word is how a broken gate looks like no gate.
            other => {
                if let Err(e) = other {
                    tracing::error!(error=%e, "queue gate read failed; dispatching anyway");
                }
                return QueueGate::Open;
            }
        };
        if row
            .get::<Option<serde_json::Value>, _>("paused")
            .and_then(|v| serde_json::from_value::<bool>(v).ok())
            .unwrap_or(false)
        {
            return QueueGate::Paused;
        }

        let Some(cap_usd) = row
            .get::<Option<serde_json::Value>, _>("budget")
            .and_then(|v| serde_json::from_value::<f64>(v).ok())
            .filter(|c| *c > 0.0)
        else {
            return QueueGate::Open; // No cap set: the common case, one query.
        };

        let spent_today = self.spent_today().await;
        if spent_today >= cap_usd {
            QueueGate::OverBudget { spent_today, cap_usd }
        } else {
            QueueGate::Open
        }
    }

    /// Spend since local midnight, matching the day buckets the activity view
    /// charts so the two can never disagree about what "today" cost.
    pub async fn spent_today(&self) -> f64 {
        sqlx::query_scalar::<_, Option<f64>>(
            "SELECT SUM(cost_usd) FROM runs WHERE created_at >= date_trunc('day', now())",
        )
        .fetch_one(&self.db.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(0.0)
    }

    /// Stop or resume dispatching. Runs already executing are left alone —
    /// pausing is about not spending more, not about throwing away work in
    /// progress.
    pub async fn set_queue_paused(&self, paused: bool) -> anyhow::Result<()> {
        self.put_setting("queue_paused", serde_json::json!(paused)).await
    }

    /// How much freedom a new task gets when the caller doesn't say.
    ///
    /// Stored rather than hard-coded because "ask me before every command"
    /// and "just get on with it" are both legitimate, and which one you want
    /// is a property of how much you trust the isolation — not of the code.
    pub async fn default_permission_mode(&self) -> PermissionMode {
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT value FROM settings WHERE key = 'default_permission_mode'",
        )
        .fetch_optional(&self.db.pool)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<PermissionMode>(v).ok())
        .unwrap_or_default()
    }

    /// How hard the model thinks, when nothing more specific says.
    ///
    /// `None` is a real answer and the default one: it leaves each CLI on its
    /// own built-in behaviour rather than aichip picking a number on the user's
    /// behalf. A stored value is an explicit choice to override that.
    pub async fn default_effort(&self) -> Option<ReasoningEffort> {
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT value FROM settings WHERE key = 'default_effort'",
        )
        .fetch_optional(&self.db.pool)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.as_str().and_then(ReasoningEffort::parse))
    }

    /// `None` clears it, returning every run to its CLI's own default — the
    /// same shape as removing the budget cap rather than setting it to zero.
    pub async fn set_default_effort(&self, effort: Option<ReasoningEffort>) -> anyhow::Result<()> {
        match effort {
            Some(e) => {
                self.put_setting("default_effort", serde_json::json!(e.as_str()))
                    .await
            }
            None => {
                sqlx::query("DELETE FROM settings WHERE key = 'default_effort'")
                    .execute(&self.db.pool)
                    .await?;
                Ok(())
            }
        }
    }

    pub async fn set_default_permission_mode(&self, mode: PermissionMode) -> anyhow::Result<()> {
        self.put_setting("default_permission_mode", serde_json::to_value(mode)?)
            .await
    }

    pub async fn daily_budget(&self) -> Option<f64> {
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT value FROM settings WHERE key = 'daily_budget_usd'",
        )
        .fetch_optional(&self.db.pool)
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<f64>(v).ok())
        .filter(|c| *c > 0.0)
    }

    /// `None` removes the cap. A zero or negative cap would mean "never run
    /// anything", which is what the pause is for, so it is stored as no cap.
    pub async fn set_daily_budget(&self, cap: Option<f64>) -> anyhow::Result<()> {
        match cap.filter(|c| *c > 0.0) {
            Some(cap) => self.put_setting("daily_budget_usd", serde_json::json!(cap)).await,
            None => {
                sqlx::query("DELETE FROM settings WHERE key = 'daily_budget_usd'")
                    .execute(&self.db.pool)
                    .await?;
                Ok(())
            }
        }
    }

    async fn put_setting(&self, key: &str, value: serde_json::Value) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.db.pool)
        .await?;
        Ok(())
    }

    async fn claim_next(&self) -> anyhow::Result<Option<Uuid>> {
        let gate = self.queue_gate().await;
        // On the edge, never on the state: `claim_next` runs every 750ms, so
        // firing on "is over budget" would be a notification every three
        // quarters of a second until midnight.
        let over = matches!(gate, QueueGate::OverBudget { .. });
        if over != self.announced_over_budget.swap(over, Ordering::SeqCst) && over {
            let spent = match &gate {
                QueueGate::OverBudget { spent_today, cap_usd } => {
                    format!("${spent_today:.2} of ${cap_usd:.2} spent — the queue is holding until midnight")
                }
                _ => String::new(),
            };
            crate::attention::fire(
                &self.db,
                crate::attention::Event::OverBudget,
                crate::attention::Ctx {
                    title: "aichip: daily budget reached".to_string(),
                    body: spent,
                    ..Default::default()
                },
            )
            .await;
        }
        if !matches!(gate, QueueGate::Open) {
            return Ok(None);
        }
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
        let row = sqlx::query(
            "SELECT status, chat_id, workflow_id, team_id, comment_id, kb_brief, research_id
             FROM runs WHERE id=$1",
        )
            .bind(run_id)
            .fetch_one(&self.db.pool)
            .await?;
        // The guard this never had. `execute` reads only the columns that
        // decide *which kind* of run it is, so a queue row that outlived its
        // run — cancelled, already finished, or claimed twice — dispatched a
        // second engine against it and was charged for. Anything not still
        // waiting to start is dropped on the floor here, which is the correct
        // response to a row that should not exist.
        let status: String = row.get("status");
        match RunStatus::parse(&status) {
            Some(s) if s.is_dispatchable() => {}
            _ => {
                tracing::warn!(%run_id, %status, "dropped a queued run that is no longer waiting");
                return Ok(());
            }
        }
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
            // Before the kb arm: the two columns are disjoint today, but if
            // any future path ever stamps KB columns onto a research run, the
            // more specific kind must win rather than lean on an invariant
            // nothing enforces.
            _ if row.get::<Option<Uuid>, _>("research_id").is_some() => {
                self.execute_research_run(run_id).await
            }
            _ if row.get::<Option<String>, _>("kb_brief").is_some() => {
                self.execute_kb_run(run_id).await
            }
            _ => self.execute_task_run(run_id).await,
        }
    }

    async fn execute_task_run(self: &Arc<Self>, run_id: Uuid) -> anyhow::Result<()> {
        let run = sqlx::query(
            // A bake-off variant overrides the card: its own agent, its own
            // tier, its own worktree. COALESCE keeps every ordinary run on
            // exactly the path it was on before.
            "SELECT r.id, r.engine, r.session_id, r.session_engine, t.id AS task_id,
                    COALESCE(r.prompt_override, t.prompt) AS prompt,
                    t.model_tier, r.tier_override,
                    COALESCE(r.agent_id, t.agent_id) AS agent_id,
                    r.variant_label, r.worktree_path AS run_worktree,
                    t.permission_mode, t.effort AS task_effort,
                    t.title, t.worktree_path, t.branch, t.chat_id AS task_chat_id,
                    r.plan_approval, r.plan_approved_at,
                    p.id AS project_id, p.path AS project_path, p.default_branch, p.full_auto_opt_in,
                    p.vcs,
                    a.system_prompt AS agent_prompt, a.model_tier AS agent_tier,
                    a.effort AS agent_effort,
                    a.allowed_tools AS agent_tools, a.permission_preset AS agent_preset,
                    a.name AS agent_name, t.skill_id
             FROM runs r
             JOIN tasks t ON t.id = r.task_id
             JOIN projects p ON p.id = t.project_id
             LEFT JOIN agents a ON a.id = COALESCE(r.agent_id, t.agent_id)
             WHERE r.id = $1",
        )
        .bind(run_id)
        .fetch_one(&self.db.pool)
        .await?;

        self.set_status(run_id, RunStatus::Starting).await?;

        let engine_id: String = run.get("engine");
        // A session id is only meaningful to the engine that minted it.
        // Handing an OpenCode `ses_…` to Claude doesn't error — it starts
        // over with none of the context, which is the quiet kind of wrong.
        let resume_session_id: Option<String> = run
            .get::<Option<String>, _>("session_id")
            .filter(|_| {
                run.get::<Option<String>, _>("session_engine").as_deref() == Some(&engine_id)
            });
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
        // A bake-off variant gets its own checkout, keyed by run rather than
        // task — the whole point is that the attempts don't see each other,
        // and sharing the task's worktree would have them overwrite one
        // another's answer.
        let variant: Option<String> = run.get("variant_label");
        let existing = match &variant {
            Some(_) => run.get::<Option<String>, _>("run_worktree"),
            None => run.get::<Option<String>, _>("worktree_path"),
        };
        let cwd = match existing {
            Some(p) => PathBuf::from(p),
            None if in_place => project_path.clone(),
            None => {
                let title: String = run.get("title");
                let (key, slug) = match &variant {
                    Some(label) => (run_id, format!("{}-{}", slugify(&title), slugify(label))),
                    None => (task_id, slugify(&title)),
                };
                let wt = self
                    .worktrees
                    .create(&project_path, &default_branch, key, &slug)
                    .await?;
                if variant.is_some() {
                    sqlx::query("UPDATE runs SET worktree_path=$1 WHERE id=$2")
                        .bind(wt.path.to_string_lossy().as_ref())
                        .bind(run_id)
                        .execute(&self.db.pool)
                        .await?;
                } else {
                    sqlx::query("UPDATE tasks SET worktree_path=$1, branch=$2 WHERE id=$3")
                        .bind(wt.path.to_string_lossy().as_ref())
                        .bind(&wt.branch)
                        .bind(task_id)
                        .execute(&self.db.pool)
                        .await?;
                }
                wt.path
            }
        };

        // Agent binding: a bound agent overrides tier, system prompt,
        // allowed tools, and permission preset.
        let agent_prompt: Option<String> = run.get("agent_prompt");
        let agent_tier: Option<String> = run.get("agent_tier");
        // Resolved below, once the tier is known — see `resolve_effort` for
        // the order these two sit in and what they fall through to.
        let agent_effort = run
            .get::<Option<String>, _>("agent_effort")
            .and_then(|e| ReasoningEffort::parse(&e));
        let card_effort = run
            .get::<Option<String>, _>("task_effort")
            .and_then(|e| ReasoningEffort::parse(&e));
        let agent_tools: Option<Vec<String>> = run.get("agent_tools");
        let agent_preset: Option<String> = run.get("agent_preset");

        // Normally a bound agent's tier wins over the card's. A bake-off
        // variant inverts that — comparing tiers means the variant's tier has
        // to beat the agent's, or every variant would run the same model.
        //
        // Resolved further down rather than here, because `auto` needs to know
        // which pass this is: writing a plan and carrying out an approved one
        // are different jobs and should not draw the same model.
        let (tier_str, tier_source) = match run.get::<Option<String>, _>("tier_override") {
            Some(explicit) => (explicit, "variant"),
            None => match agent_tier {
                Some(t) => (t, "agent"),
                None => (run.get("model_tier"), "card"),
            },
        };
        // Most specific wins: the agent's own preset, else the card's, else
        // the workspace default. Both of the first two are nullable and mean
        // "inherit" when unset — resolved here rather than frozen at create
        // time, so changing the default reaches work already in the backlog.
        let permission_mode = match agent_preset
            .or_else(|| run.get::<Option<String>, _>("permission_mode"))
            .and_then(|m| serde_json::from_value::<PermissionMode>(serde_json::Value::String(m)).ok())
        {
            Some(explicit) => explicit,
            None => self.default_permission_mode().await,
        };

        // Plan-first: write the plan, park, and let a person approve or
        // rewrite it before anything changes. Both halves are ordinary runs of
        // this same function — the phase decides what gets asked for.
        let task_prompt: String = run.get("prompt");
        let stored_plan = self.stored_plan(run_id).await?;
        let phase = task_plan::decide(
            run.get("plan_approval"),
            match stored_plan.as_deref() {
                Some(text) => task_plan::PlanStep::Written(text),
                None => task_plan::PlanStep::Missing,
            },
            run.get::<Option<chrono::DateTime<chrono::Utc>>, _>("plan_approved_at")
                .is_some(),
        );
        let planning = phase == task_plan::Phase::Plan;

        // Now the phase is known, settle the tier. `auto` means aichip picks;
        // anything else is a person's choice and is left exactly alone.
        //
        // The decision is recorded on the run below, before the process
        // starts. A router that changed which model ran your work without
        // saying so would be the same silent downgrade this codebase refuses
        // elsewhere — the reason has to survive to the card.
        let (tier, tier_source, tier_rule, tier_reason) =
            match TierChoice::parse(&tier_str).unwrap_or(TierChoice::Medium) {
                TierChoice::Auto => {
                    let signals = self.tier_signals(run_id, task_id, &run).await;
                    let phase = match (&phase, planning) {
                        (_, true) => aichip_shared::TierPhase::Plan,
                        (task_plan::Phase::Work { plan: Some(_) }, _) => {
                            aichip_shared::TierPhase::Work
                        }
                        _ => aichip_shared::TierPhase::Single,
                    };
                    let d = aichip_shared::classify_tier(&signals, phase);
                    tracing::info!(%run_id, tier = ?d.tier, rule = d.rule, "auto tier");
                    (d.tier, "auto", Some(d.rule.to_string()), Some(d.because))
                }
                fixed => (
                    // `unwrap_or_default` is safe here only because `Auto` is
                    // handled above: `fixed()` is `None` for that case alone.
                    fixed.fixed().unwrap_or_default(),
                    tier_source,
                    None,
                    None,
                ),
            };
        let effort = self
            .resolve_effort(agent_effort, card_effort, &engine_id, tier)
            .await;

        // Capability gate. Checked here as well as at enqueue time because a
        // card's mode can be edited after it was queued — and a run that
        // can't honour its mode must fail loudly rather than quietly running
        // with more freedom than was asked for.
        if let Err(reason) = aichip_engines::vet(
            engine.as_ref(),
            permission_mode,
            resume_session_id.is_some(),
        ) {
            // Persist the reason as an event, not just a status, so it shows
            // up in the run's transcript where the user is already looking.
            let seq = next_seq(&self.db, run_id).await?;
            self.persist_and_publish(run_id, None, seq, &AichipEvent::RunFailed {
                reason: reason.clone(),
            })
            .await?;
            self.finish(run_id, RunStatus::Failed, Some(reason)).await?;
            return Ok(());
        }

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

        // Servers this agent opted into. Loaded before the config is written
        // because they go into the same file as aichip's own endpoint.
        let bound_agent: Option<Uuid> = run.get("agent_id");
        let user_servers = crate::mcp_servers::for_agent(&self.db, bound_agent)
            .await
            .unwrap_or_else(|e| {
                // A broken server row must not take the whole run down; the
                // agent simply runs without the extra capability.
                tracing::warn!(%run_id, error=%e, "could not load agent MCP servers");
                vec![]
            });

        // No engine-id check: every engine that can do MCP now gets the same
        // wiring, and each adapter renders it. The old `"claude-code"` match
        // silently handed any other engine an empty config.
        let mcp = McpWiring {
            aichip_url: self
                .mcp_base_url
                .as_ref()
                .map(|b| format!("{b}/mcp/run/{run_id}")),
            servers: user_servers.iter().map(|s| s.to_spec()).collect(),
        };

        // An empty allow-list means "no --allowedTools flag", which allows
        // everything — so only extend a list that already exists. Appending
        // to an empty one would silently narrow the run to MCP tools alone.
        let mut allowed_tools = agent_tools.unwrap_or_default();
        if !allowed_tools.is_empty() {
            allowed_tools.extend(user_servers.iter().map(|s| s.tool_prefix()));
        }

        let model_id = self.model_for(&engine_id, tier);
        // The tier lands on the row in the same write as the model, so the
        // choice is durable *before* the process starts rather than inferred
        // afterwards. `model` alone could not answer it: two tiers can map to
        // one model and the mapping is editable, so a model id read back next
        // week cannot say which tier asked for it, or who decided.
        sqlx::query(
            "UPDATE runs SET model=$1, started_at=now(),
                             tier_resolved=$3, tier_source=$4, tier_rule=$5, tier_reason=$6
             WHERE id=$2",
        )
        .bind(&model_id)
        .bind(run_id)
        .bind(TierChoice::from(tier).as_str())
        .bind(tier_source)
        .bind(&tier_rule)
        .bind(&tier_reason)
        .execute(&self.db.pool)
        .await?;

        // Attachments are folded in here rather than at the route, so that
        // re-running a task re-attaches and `tasks.prompt` keeps holding
        // exactly what the user typed.
        let atts = attachments::for_task(&self.db, task_id).await.unwrap_or_default();
        // What the run is actually asked for depends on the phase: draft a
        // plan, or carry out the one that was approved. Attachments are folded
        // into either, since a spec you were given is context for planning too.
        let asked = match &phase {
            task_plan::Phase::Plan => match self.plan_revision_note(run_id).await? {
                Some(note) => task_plan::revise_prompt(
                    &task_prompt,
                    stored_plan.as_deref().unwrap_or(""),
                    &note,
                ),
                None => task_plan::plan_prompt(&task_prompt),
            },
            task_plan::Phase::Work { plan: Some(plan) } => task_plan::work_prompt(
                &task_prompt,
                plan,
                // Whether a human rewrote it, not whether it merely differs.
                self.plan_was_edited(run_id).await?,
            ),
            task_plan::Phase::Work { plan: None } => task_prompt.clone(),
        };
        let (prompt, extra_read_dirs) = attachments::augment_prompt(&asked, &atts);
        // Knowledge-base articles tagged onto the card. This is what makes
        // tagging one worth doing: the agent is handed the runbook rather than
        // left to infer it from the code.
        let articles = crate::kb::for_run(&self.db, Some(task_id), None)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(%run_id, error = %e, "could not load tagged articles");
                vec![]
            });
        let prompt = crate::kb::augment_prompt(&prompt, &articles);

        // A bound agent carries its memory into the run: what it did on this
        // project before is context, and what it does now becomes memory below.
        let project_id: Uuid = run.get("project_id");

        // What the person who owns this project wants every run to know, and
        // the skill this card was created with — background then method. Last,
        // so the request and its attachments are read first and these are what
        // they sit against; and in the *prompt* rather than the system prompt,
        // because standing context that outranked the request is the failure
        // both are fenced against.
        //
        // This was two calls spelled out here and nowhere else, which is how
        // a $75 team card ran with none of it. `Standing::apply` is those two
        // calls in that order and is pinned byte-identical to them.
        let standing = crate::runs::context::Standing::load(
            &self.db,
            Some(project_id),
            run.get::<Option<Uuid>, _>("skill_id"),
        )
        .await;
        let prompt = standing.apply(&prompt);
        let memory_block = match bound_agent {
            Some(agent_id) => memory::recall(&self.db, agent_id, Some(project_id))
                .await
                .ok()
                .as_deref()
                .and_then(memory::render),
            None => None,
        };

        // The work pass picks up the planning session, so the agent keeps
        // everything it learned reading the code — but only from an engine
        // that minted it and only if that engine can resume at all.
        let resume_session_id = match (&phase, engine.capabilities().resume_sessions) {
            (task_plan::Phase::Work { plan: Some(_) }, true) => self
                .plan_session(run_id)
                .await?
                .filter(|(_, minted_by)| minted_by == &engine_id)
                .map(|(sid, _)| sid)
                .or(resume_session_id),
            _ => resume_session_id,
        };

        let tool_timeout_ms = self.mcp_tool_timeout_ms().await;
        let spec = RunSpec {
            cwd,
            prompt,
            model_tier: tier,
            model_id,
            effort,
            resume_session_id,
            // Planning is read-only whatever the card says. A plan you are
            // going to be asked to approve is worthless if the work already
            // happened while it was being written — and with nothing to
            // approve, nothing can prompt either.
            permission_mode: if planning {
                PermissionMode::AutoEdit
            } else {
                permission_mode
            },
            allowed_tools: if planning {
                task_plan::PLANNING_TOOLS.iter().map(|t| t.to_string()).collect()
            } else {
                allowed_tools
            },
            denied_tools: if planning {
                task_plan::PLANNING_DENIED.iter().map(|t| t.to_string()).collect()
            } else {
                vec![]
            },
            append_system_prompt: match (agent_prompt.filter(|p| !p.is_empty()), memory_block) {
                (Some(p), Some(m)) => Some(format!("{p}{m}")),
                (Some(p), None) => Some(p),
                // Memory is useful even when the agent has no system prompt.
                (None, Some(m)) => Some(m),
                (None, None) => None,
            },
            mcp,
            run_key: run_id.to_string(),
            extra_read_dirs,
            // Nothing to approve during planning, so nothing to ask about.
            permission_prompt_tool: !planning,
            extra_env: HashMap::from([
                ("AICHIP_RUN_ID".to_string(), run_id.to_string()),
                // Permission prompts block the MCP tools/call until the user
                // answers in the dashboard, so the CLI has to be willing to
                // wait exactly as long as the broker is. See
                // `mcp_tool_timeout_ms` — one value, three call sites.
                ("MCP_TOOL_TIMEOUT".to_string(), tool_timeout_ms.clone()),
                // Server startup. 60s was ample when the only server was
                // aichip's own local HTTP endpoint; a user-connected server
                // may cold-start `npx -y …` and fetch a package first, and a
                // server that misses this window fails every tool call after
                // it with an unhelpful "operation timed out".
                ("MCP_TIMEOUT".to_string(), "180000".to_string()),
            ]),
        };

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        // Planning doesn't finalize: a completed *plan* is not a completed
        // run, and letting `stream_run` mark it terminal would send the card
        // to review with nothing done.
        let outcome = self
            .stream_run(run_id, None, &seq, engine, spec, if planning { CallerKind::TaskPlanning } else { CallerKind::TaskWork })
            .await?;

        // A held run is coming back; it has not ended. `park_for_approval`
        // reads "not completed" as "no plan to approve" and fails the run,
        // which would undo the hold `hold_rate_limited` just wrote — status,
        // queue row and all — one line after writing it.
        if outcome.status == RunStatus::RateLimited {
            return Ok(());
        }

        if planning {
            return self.park_for_approval(run_id, &outcome).await;
        }

        // Only the completed case here. A run that ended badly has already had
        // its card moved off In Progress by `finish`, which is the one place
        // every ending goes through — including the ones that never reach this
        // function at all.
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

        // An app's own change lands by itself. A no-op for every other card,
        // and best-effort like the memory write above: a build that failed to
        // merge must not turn a completed run into a failed one — the build row
        // records what happened and the card keeps its diff.
        if let Err(e) = apps::build::settle(
            &self.db,
            &self.worktrees,
            task_id,
            outcome.status,
            outcome.reason.as_deref(),
        )
        .await
        {
            tracing::warn!(%run_id, error = %e, "an app's change did not land");
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

    /// Queue a run that writes documentation instead of code.
    ///
    /// `article_id` present means "revise this one"; absent means a new draft.
    pub async fn enqueue_kb_article(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        brief: &str,
        engine: Option<&str>,
        article_id: Option<Uuid>,
        parent_id: Option<Uuid>,
    ) -> anyhow::Result<Uuid> {
        let engine = engine
            .map(str::to_string)
            .unwrap_or_else(|| self.default_engine());
        if self.engine(&engine).is_none() {
            anyhow::bail!("{engine} isn't installed on this machine");
        }
        // A new article's row is created up front, so the editor has something
        // to open the moment the run is queued rather than only once it lands.
        let article_id = match article_id {
            Some(id) => id,
            None => sqlx::query_scalar(
                "INSERT INTO kb_articles
                    (workspace_id, project_id, parent_id, title, status, origin, position)
                 VALUES ($1, $2, $3, $4, 'draft', 'agent',
                         COALESCE((SELECT max(position) + 1000 FROM kb_articles
                                    WHERE workspace_id = $1
                                      AND parent_id IS NOT DISTINCT FROM $3), 1000))
                 RETURNING id",
            )
            .bind(workspace_id)
            .bind(project_id)
            .bind(parent_id)
            .bind(crate::kb::sanitize::summarize(brief, 80))
            .fetch_one(&self.db.pool)
            .await?,
        };

        let run_id: Uuid = sqlx::query_scalar(
            "INSERT INTO runs (status, trigger, engine, kb_article_id, kb_brief, kb_project_id)
             VALUES ('queued', 'kb', $1, $2, $3, $4) RETURNING id",
        )
        .bind(&engine)
        .bind(article_id)
        .bind(brief)
        .bind(project_id)
        .fetch_one(&self.db.pool)
        .await?;
        sqlx::query("UPDATE kb_articles SET source_run_id=$1 WHERE id=$2")
            .bind(run_id)
            .bind(article_id)
            .execute(&self.db.pool)
            .await?;
        self.queue(run_id, 9).await?;
        Ok(run_id)
    }

    /// Write the article, then store it.
    ///
    /// Read-only over the project itself — no worktree, no branch. Nothing is
    /// being changed, so there is nothing to isolate or review, and creating a
    /// worktree for a run that only reads would leave litter behind.
    async fn execute_kb_run(self: &Arc<Self>, run_id: Uuid) -> anyhow::Result<()> {
        let row = sqlx::query(
            "SELECT r.engine, r.kb_brief, r.kb_article_id, p.path AS project_path,
                    a.title, a.content_html, a.current_seq, p.id AS project_id
             FROM runs r
             JOIN projects p ON p.id = r.kb_project_id
             LEFT JOIN kb_articles a ON a.id = r.kb_article_id
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

        let brief: String = row.get::<Option<String>, _>("kb_brief").unwrap_or_default();
        let existing: String = row.get::<Option<String>, _>("content_html").unwrap_or_default();
        // The revision the agent is working from, captured before it starts.
        // If a person edits the page while it runs, the review UI compares
        // against *this* and can say the page moved on underneath it — rather
        // than silently presenting a stale rewrite as an up-to-date one.
        let base_seq: i32 = row.get::<Option<i32>, _>("current_seq").unwrap_or(0);
        // Brain, and no skill. The job is "describe this repository
        // accurately", and the Brain is a hand-written list of exactly the
        // facts a generated page gets wrong. This is also the one prompt in
        // the codebase whose *output* becomes input to other prompts —
        // `kb::for_run` feeds published pages back into task runs as reference
        // — so a page written without the facts is wrong repeatedly, not once.
        //
        // No skill because nobody named one: a generated article was never
        // asked to be written a particular way, and inferring one is the thing
        // Skills exist not to do.
        let standing =
            crate::runs::context::Standing::brain_only(&self.db, Some(row.get("project_id"))).await;
        let context = standing.block();
        let prompt = if existing.trim().is_empty() {
            crate::kb::write::prompt(&brief, &context)
        } else {
            crate::kb::write::rewrite_prompt(
                &brief,
                &row.get::<Option<String>, _>("title").unwrap_or_default(),
                &existing,
                &context,
            )
        };

        let tier = ModelTier::Medium;
        let model_id = self.model_for(&engine_id, tier);
        sqlx::query("UPDATE runs SET model=$1, started_at=now() WHERE id=$2")
            .bind(&model_id)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;

        let spec = RunSpec {
            cwd: PathBuf::from(row.get::<String, _>("project_path")),
            prompt,
            model_tier: tier,
            model_id,
            effort: None,
            resume_session_id: None,
            permission_mode: PermissionMode::AutoEdit,
            allowed_tools: crate::kb::write::TOOLS.iter().map(|t| t.to_string()).collect(),
            denied_tools: crate::kb::write::DENIED.iter().map(|t| t.to_string()).collect(),
            append_system_prompt: None,
            mcp: McpWiring::default(),
            run_key: run_id.to_string(),
            extra_read_dirs: vec![],
            permission_prompt_tool: false,
            extra_env: HashMap::from([("AICHIP_RUN_ID".to_string(), run_id.to_string())]),
        };

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let outcome = self.stream_run(run_id, None, &seq, engine, spec, CallerKind::KbGeneration).await?;
        if outcome.status != RunStatus::Completed {
            return Ok(());
        }

        let article_id: Uuid = row.get("kb_article_id");
        let prepared =
            crate::kb::render::prepare(&crate::kb::write::extract_html(&outcome.output));
        if prepared.html.trim().is_empty() {
            // A brand-new page with nothing in it is litter, not a draft:
            // remove the placeholder rather than leaving an empty row in the
            // tree that nobody can tell apart from a page someone meant to
            // start. An existing page is left exactly as it was.
            //
            // `base_seq == 0` alone is not enough to call a page empty — a page
            // whose body exists but whose revision log doesn't yet (a backfill
            // that failed) also reads as 0, and deleting that would destroy
            // somebody's writing. The body is the authority.
            if base_seq == 0 && existing.trim().is_empty() {
                sqlx::query("DELETE FROM kb_articles WHERE id=$1")
                    .bind(article_id)
                    .execute(&self.db.pool)
                    .await?;
            }
            self.finish(
                run_id,
                RunStatus::Failed,
                Some("the agent produced no article".into()),
            )
            .await?;
            return Ok(());
        }

        let title = crate::kb::write::title_from(&prepared.html, &brief);
        let rev = crate::kb::revisions::NewRevision {
            title: &title,
            html: &prepared.html,
            text: &prepared.text,
            author: crate::kb::revisions::Author::Agent,
            kind: "agent",
            base_seq: Some(base_seq),
            run_id: Some(run_id),
            note: "",
        };

        if base_seq == 0 && existing.trim().is_empty() {
            // Nothing to overwrite and nobody to ask: a page that was created
            // by asking an agent to write it *is* the agent's page, and making
            // someone approve a proposal against a blank page is ceremony
            // with no decision in it. It stays a draft either way.
            //
            // Guarded on the body as well as the pointer, so a page that has
            // content but no revision row is treated as somebody's work rather
            // than as an empty placeholder to write over.
            //
            // `Some(0)` rather than `None`, because `existing` was read long
            // before this line — a whole agent run ago. If somebody started
            // typing on the placeholder while the run was in flight the page is
            // no longer blank, and this refuses instead of erasing them.
            crate::kb::revisions::save_edit(&self.db, article_id, rev, Some(0)).await?;
        } else {
            // Everything else is a proposal. This is the rule the whole
            // revision log exists for: an agent must never replace a body a
            // person may have written, with no copy and no diff.
            crate::kb::revisions::propose(&self.db, article_id, rev).await?;
        }
        Ok(())
    }

    /// Queue a deep-research run for a question about a project.
    ///
    /// Returns `(research_id, run_id)`. A re-run of an existing research goes
    /// through `enqueue_research_run` instead, which reuses the row.
    pub async fn enqueue_research(
        &self,
        // A project, or a workspace for a *general* research — one with no
        // repository behind it, answered from the web alone. Exactly one; the
        // table CHECKs the same rule.
        project_id: Option<Uuid>,
        workspace_id: Option<Uuid>,
        question: &str,
        engine: Option<&str>,
        // NULL means the research defaults: Complex, with the operator's
        // effort for that tier. Stored on the research so a re-run asks the
        // same way.
        model_tier: Option<ModelTier>,
        effort: Option<ReasoningEffort>,
    ) -> anyhow::Result<(Uuid, Uuid)> {
        if project_id.is_none() && workspace_id.is_none() {
            anyhow::bail!("a research belongs to a project or to a workspace");
        }
        let engine = engine
            .map(str::to_string)
            .unwrap_or_else(|| self.default_engine());
        if self.engine(&engine).is_none() {
            anyhow::bail!("{engine} isn't installed on this machine");
        }
        let research_id: Uuid = sqlx::query_scalar(
            "INSERT INTO researches (project_id, workspace_id, question, model_tier, effort)
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(project_id)
        .bind(workspace_id)
        .bind(question)
        .bind(model_tier.map(|t| TierChoice::from(t).as_str().to_string()))
        .bind(effort.map(|e| e.as_str().to_string()))
        .fetch_one(&self.db.pool)
        .await?;
        let run_id = self.enqueue_research_run(research_id, &engine).await?;
        Ok((research_id, run_id))
    }

    /// A (re-)run against an existing research. Its own function because
    /// re-running is a fresh runs row against the same question — the report
    /// is replaced wholesale on completion, never merged.
    pub async fn enqueue_research_run(
        &self,
        research_id: Uuid,
        engine: &str,
    ) -> anyhow::Result<Uuid> {
        if self.engine(engine).is_none() {
            anyhow::bail!("{engine} isn't installed on this machine");
        }
        let run_id: Uuid = sqlx::query_scalar(
            "INSERT INTO runs (status, trigger, engine, research_id)
             VALUES ('queued', 'research', $1, $2) RETURNING id",
        )
        .bind(engine)
        .bind(research_id)
        .fetch_one(&self.db.pool)
        .await?;
        // Above task runs (10), below comment replies (15): a person is
        // sitting on the Research page watching, but the wait is measured in
        // minutes either way.
        self.queue(run_id, 12).await?;
        Ok(run_id)
    }

    /// Investigate a question: read-only over the project's real checkout,
    /// plus the CLI's own web search. Same shape as `execute_kb_run` — no
    /// worktree, nothing to isolate — with two deliberate differences, both
    /// commented at the site.
    async fn execute_research_run(self: &Arc<Self>, run_id: Uuid) -> anyhow::Result<()> {
        // LEFT JOIN: a *general* research has no project — it is answered
        // from the web alone, out of a scratch directory.
        let row = sqlx::query(
            "SELECT r.engine, rs.id AS research_id, rs.question,
                    rs.model_tier AS research_tier, rs.effort AS research_effort,
                    p.path AS project_path, p.id AS project_id
             FROM runs r
             JOIN researches rs ON rs.id = r.research_id
             LEFT JOIN projects p ON p.id = rs.project_id
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

        let research_id: Uuid = row.get("research_id");
        let question: String = row.get("question");
        let project_id: Option<Uuid> = row.get("project_id");
        // The two shapes differ in everything the project used to supply:
        // where to stand, what to read, and which facts come along.
        //
        // Brain (project case only), no skill — the same reasoning as KB
        // generation: the Brain is a hand-written list of exactly the facts
        // an investigation gets wrong, and nobody named a skill.
        let (cwd, prompt, tools) = match (project_id, row.get::<Option<String>, _>("project_path"))
        {
            (Some(pid), Some(path)) => {
                let standing =
                    crate::runs::context::Standing::brain_only(&self.db, Some(pid)).await;
                (
                    PathBuf::from(path),
                    crate::runs::research::prompt(&question, &standing.block()),
                    crate::runs::research::TOOLS,
                )
            }
            _ => {
                // A scratch directory, the utility-run precedent — the agent
                // has to stand somewhere, and it must not be anywhere with
                // files worth reading.
                let scratch = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".aichip")
                    .join("tmp");
                tokio::fs::create_dir_all(&scratch).await?;
                (
                    scratch,
                    crate::runs::research::web_prompt(&question),
                    crate::runs::research::WEB_TOOLS,
                )
            }
        };

        // Difference one from KB: the person's choice, with Complex as the
        // default — research is the thinking-heavy kind of run — and the
        // effort actually resolved rather than hardcoded, so the operator's
        // per-tier setting applies when nothing was picked.
        let tier = row
            .get::<Option<String>, _>("research_tier")
            .and_then(|t| TierChoice::parse(&t))
            .and_then(TierChoice::fixed)
            .unwrap_or(ModelTier::Complex);
        let effort = self
            .resolve_effort(
                None,
                row.get::<Option<String>, _>("research_effort")
                    .and_then(|e| ReasoningEffort::parse(&e)),
                &engine_id,
                tier,
            )
            .await;
        let model_id = self.model_for(&engine_id, tier);
        sqlx::query("UPDATE runs SET model=$1, started_at=now() WHERE id=$2")
            .bind(&model_id)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;

        let spec = RunSpec {
            cwd,
            prompt,
            model_tier: tier,
            model_id,
            effort,
            resume_session_id: None,
            // Difference two: not KB's hardcoded AutoEdit. Research grants
            // web tools, and an engine that cannot pause to ask (OpenCode)
            // answers Reviewed by rejecting every call — the same reason chat
            // resolves this per capability. The denials still bind either way.
            permission_mode: chat_permission_mode(engine.as_ref()),
            allowed_tools: tools.iter().map(|t| t.to_string()).collect(),
            denied_tools: crate::runs::research::DENIED.iter().map(|t| t.to_string()).collect(),
            append_system_prompt: None,
            mcp: McpWiring::default(),
            run_key: run_id.to_string(),
            extra_read_dirs: vec![],
            permission_prompt_tool: false,
            extra_env: HashMap::from([("AICHIP_RUN_ID".to_string(), run_id.to_string())]),
        };

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let outcome = self
            .stream_run(run_id, None, &seq, engine, spec, CallerKind::Research)
            .await?;
        if outcome.status != RunStatus::Completed {
            return Ok(());
        }

        let report = crate::runs::research::extract_report(&outcome.output);
        if report.trim().is_empty() {
            // Unlike KB there is no placeholder to clean up: the researches
            // row keeps the question, and the page offers a re-run.
            self.finish(run_id, RunStatus::Failed, Some("the agent produced no report".into()))
                .await?;
            return Ok(());
        }
        // Scrubbed at write time, not at save-to-KB: the report quotes web
        // content — third-party text — and later becomes a KB page quoted
        // into future prompts. The same reason `rewrite_prompt` scrubs the
        // article it is handed.
        let report = crate::fence::scrub_foreign(&report, &[]);
        let title = crate::runs::research::title_from(&report, &question);
        sqlx::query(
            "UPDATE researches SET report_md=$1, title=$2, updated_at=now() WHERE id=$3",
        )
        .bind(&report)
        .bind(&title)
        .bind(research_id)
        .execute(&self.db.pool)
        .await?;
        Ok(())
    }

    async fn execute_chat_run(self: &Arc<Self>, run_id: Uuid, chat_id: Uuid) -> anyhow::Result<()> {
        let row = sqlx::query(
            // Lateral join rather than a scalar subquery: the turn needs the
            // message's id as well as its text, to look up its attachments.
            "SELECT r.engine, c.session_id, c.session_engine, c.model_tier AS chat_tier,
                    c.effort AS chat_effort, p.path AS project_path, p.id AS project_id,
                    p.kind AS project_kind,
                    m.id AS user_message_id, m.content AS user_message
             FROM runs r JOIN chats c ON c.id = r.chat_id
             LEFT JOIN projects p ON p.id = c.project_id
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
        // The retrieval query, captured before any augment: what the person
        // typed, not the attachment framing or mention instructions that get
        // wrapped around it below.
        let raw_user_text = user_message.clone();
        // NULL for a *general* chat — a conversation with no project behind
        // it. Everything project-shaped below is gated on this: attachments
        // and mentions are project machinery, the brain belongs to a project,
        // and the MCP task tools resolve through one.
        let project_id: Option<Uuid> = row.get("project_id");
        let project_kind: Option<String> = row.get("project_kind");

        // An attachment-only turn stores empty content, so the prompt may be
        // nothing but the attachment block.
        let atts = match row.get::<Option<Uuid>, _>("user_message_id") {
            Some(message_id) => attachments::for_message(&self.db, message_id)
                .await
                .unwrap_or_default(),
            None => vec![],
        };
        let (user_message, extra_read_dirs) = attachments::augment_prompt(&user_message, &atts);

        // Who the user named with `@`. Resolved when the message was sent, so
        // this is a lookup rather than a second parse — and `create_task` reads
        // the same rows, which is what makes the binding hold even if the model
        // forgets to pass `agent_name`.
        let mentioned: Vec<String> = mentions::latest_for_chat(&self.db, chat_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        let user_message = mentions::augment_prompt(&user_message, &mentioned);

        // And which skills. Same lookup, same reason — but a different message:
        // a mentioned skill needs no action from the assistant, only for it not
        // to stop and ask about a name it cannot find in the agent library.
        let named_skills: Vec<String> = mentions::latest_skills_for_chat(&self.db, chat_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        let user_message = mentions::augment_skills_prompt(&user_message, &named_skills);

        // And which knowledge-base pages the person attached to this turn.
        //
        // Per-message like the attachments above, and *not* like the brain
        // below: a page is chosen for a question. It therefore runs on a
        // resumed session too, which is correct — the session carries the
        // earlier turns, but it cannot carry a page that had not been picked
        // yet. The same fenced, capped block a board run gets, because these
        // bodies are written by people *and by other agents* and must read as
        // reference material rather than as instructions.
        let pages = crate::kb::for_chat(&self.db, chat_id).await.unwrap_or_default();
        let user_message = crate::kb::augment_prompt(&user_message, &pages);

        // A space chat retrieves before it answers: the top passages from the
        // documents, semantically matched against what the person just typed.
        // Per-message context, exactly like attachments — which passages
        // matter depends on this question, so it runs every turn, resumed
        // session or not (unlike the brain below, which the session already
        // carries). Any failure — empty index, embedder still downloading,
        // offline — leaves the prompt untouched: the chat must work without
        // it, it just answers from Read/Grep instead.
        let user_message = match (project_id, project_kind.as_deref()) {
            (Some(pid), Some("space")) => {
                match crate::rag::retrieve::top_k(
                    &self.db,
                    pid,
                    &raw_user_text,
                    crate::rag::retrieve::DEFAULT_K,
                )
                .await
                {
                    Ok(passages) => crate::rag::retrieve::augment_prompt(&user_message, &passages),
                    Err(e) => {
                        tracing::warn!(%run_id, error=%e, "space retrieval skipped");
                        user_message
                    }
                }
            }
            _ => user_message,
        };

        // The same standing context a board run gets. A chat that has to be
        // told where the code lives every time is the reason this feature
        // exists — and the chat is where a person asks the questions the brain
        // is written to answer.
        //
        // Only on the first turn. A chat *resumes* its session (see
        // `resume_session_id` below), so every later message is the same
        // conversation and already has this. Appending each time paid for the
        // block again per turn and stacked N copies of "read this as
        // background" into one context, which is precisely how a framing stops
        // being read as a framing — the failure the fence is there to prevent.
        //
        // Brain and *not* skill, decided rather than defaulted: chat resolves
        // `@skill` mentions by name a few lines above, and deliberately sends
        // only the name — the body travels with the card the chat creates, so
        // pasting it here would spend it twice and put the method in front of
        // a conversation that has not agreed on the job yet.
        let user_message = match (&session_id, project_id) {
            (None, Some(pid)) => {
                crate::runs::context::Standing::brain_only(&self.db, Some(pid))
                    .await
                    .apply(&user_message)
            }
            _ => user_message,
        };

        // The aichip tools all resolve through the chat's project, so a
        // general chat gets none — tools that answer every call with an
        // error about a missing project are worse than an empty toolbox.
        // Any project-attached chat gets the endpoint; *which* tools it
        // lists is decided server-side by kind, so a space sees the document
        // tools and never the board tools (whose cards would edit the
        // document folder in place).
        let is_repo = project_kind.as_deref() == Some("repo") || project_kind.as_deref() == Some("app");
        let mcp = match project_id {
            Some(_) => McpWiring {
                aichip_url: self
                    .mcp_base_url
                    .as_ref()
                    .map(|b| format!("{b}/mcp/chat/{chat_id}")),
                servers: vec![],
            },
            None => McpWiring::default(),
        };

        // Both were hardcoded — Medium, and no effort at all — which is why the
        // chat composer had a picker for the engine and nothing else. NULL still
        // means inherit, resolved now rather than frozen when the chat opened.
        //
        // `auto` is not offered for chat yet, and lands on Medium — stated
        // here rather than left to `unwrap_or_default`, because that is the
        // shape of accident this whole feature is guarding against: an
        // unrecognised tier quietly becoming the dearest ordinary model.
        let tier: ModelTier = row
            .get::<Option<String>, _>("chat_tier")
            .and_then(|t| TierChoice::parse(&t))
            .and_then(TierChoice::fixed)
            .unwrap_or(ModelTier::Medium);
        let chat_effort = self
            .resolve_effort(
                None,
                row.get::<Option<String>, _>("chat_effort")
                    .and_then(|e| ReasoningEffort::parse(&e)),
                &engine_id,
                tier,
            )
            .await;
        let model_id = self.model_for(&engine_id, tier);
        sqlx::query("UPDATE runs SET model=$1, started_at=now() WHERE id=$2")
            .bind(&model_id)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;

        // Where the assistant stands and what it may reach for. A repo chat
        // works read-only in the real checkout with the board tools; a space
        // chat stands in its document folder with the read tools and the web
        // (documents plus what they reference); a general chat stands in a
        // scratch directory with the web alone.
        let (cwd, allowed, system_prompt) = match row.get::<Option<String>, _>("project_path") {
            Some(path) if is_repo => (
                PathBuf::from(path),
                CHAT_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                CHAT_SYSTEM_PROMPT,
            ),
            Some(path) => (
                PathBuf::from(path),
                SPACE_CHAT_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                SPACE_CHAT_SYSTEM_PROMPT,
            ),
            None => {
                let scratch = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".aichip")
                    .join("tmp");
                tokio::fs::create_dir_all(&scratch).await?;
                (
                    scratch,
                    crate::runs::research::WEB_TOOLS.iter().map(|s| s.to_string()).collect(),
                    GENERAL_CHAT_SYSTEM_PROMPT,
                )
            }
        };
        let spec = RunSpec {
            cwd,
            prompt: user_message,
            model_tier: tier,
            model_id,
            effort: chat_effort,
            resume_session_id: session_id,
            permission_mode: chat_permission_mode(engine.as_ref()),
            allowed_tools: allowed,
            append_system_prompt: Some(system_prompt.to_string()),
            mcp,
            denied_tools: CHAT_DENIED_TOOLS.iter().map(|s| s.to_string()).collect(),
            run_key: run_id.to_string(),
            extra_read_dirs,
            permission_prompt_tool: false,
            extra_env: HashMap::from([("AICHIP_CHAT_ID".to_string(), chat_id.to_string())]),
        };

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let outcome = self
            .stream_run(run_id, None, &seq, engine, spec, CallerKind::Chat)
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
        // Articles referenced by the card or by this comment specifically —
        // "@agent, see #runbook" has to reach the reply, or the reference is
        // decoration.
        let articles = crate::kb::for_run(&self.db, Some(task_id), Some(comment_id))
            .await
            .unwrap_or_default();
        prompt = crate::kb::augment_prompt(&prompt, &articles);
        // The project's standing context reaches a reply too: an agent
        // answering "where does this live?" on a card should know the same
        // things as one doing the work.
        //
        // Brain, not skill: a reply answers a question, it does not do the
        // job, and this run holds only Read, Grep and Glob. A skill describes
        // how the person wants work done, which is not what is being asked
        // for here.
        prompt = crate::runs::context::Standing::brain_only(
            &self.db,
            Some(row.get::<Uuid, _>("project_id")),
        )
        .await
        .apply(&prompt);
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
        let model_id = self.model_for(&engine_id, tier);
        sqlx::query("UPDATE runs SET model=$1, started_at=now() WHERE id=$2")
            .bind(&model_id)
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;

        let system_prompt: String = row.get("system_prompt");
        // A teammate replying used to be the one path that ignored everything
        // but its own agent's budget — so a tier set to think hard did nothing
        // in the surface where several agents talk at once.
        let reply_effort = self
            .resolve_effort(
                row.get::<Option<String>, _>("agent_effort")
                    .and_then(|e| ReasoningEffort::parse(&e)),
                None,
                &engine_id,
                tier,
            )
            .await;
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
            mcp: McpWiring::default(),
            denied_tools: vec![],
            run_key: run_id.to_string(),
            extra_read_dirs: vec![],
            permission_prompt_tool: false,
            extra_env: HashMap::from([("AICHIP_RUN_ID".to_string(), run_id.to_string())]),
        };

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let outcome = self.stream_run(run_id, None, &seq, engine, spec, CallerKind::CommentReply).await?;

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
            "SELECT w.source_yaml, w.name, r.engine, r.trigger, r.task_id, p.id AS project_id,
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
        let trigger: String = row.get("trigger");
        // Resolved here purely to fail fast: a workflow naming an engine this
        // machine doesn't have should die before it creates a worktree, not
        // three steps in. Each step resolves its own below.
        self.engine(&engine_id)
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

        // Read once for the whole pipeline, not per step. A workflow is one
        // decision, and a Brain edited while a six-step run is in flight
        // should not leave the second half of it working from different
        // facts than the first.
        let standing = crate::runs::context::Standing::brain_only(
            &self.db,
            Some(row.get::<Uuid, _>("project_id")),
        )
        .await;

        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);
        let mut outputs = StepOutputs::new();
        // Session ids, tagged with the engine that minted them: a `continue`
        // step whose dependency ran elsewhere must start fresh rather than
        // hand an `ses_…` to a CLI that has never seen it.
        let mut sessions: HashMap<String, (String, String)> = HashMap::new();
        let mut failure: Option<String> = None;

        'layers: for layer in workflow.layers()? {
            for step_id in layer {
                // Same reason as the org executor: a cancel has to stop the
                // pipeline, not just whichever step was mid-flight.
                if self.cancel_requested(run_id) {
                    failure = Some("canceled".to_string());
                    break 'layers;
                }
                let step = workflow
                    .step(&step_id)
                    .ok_or_else(|| anyhow::anyhow!("missing step {step_id}"))?;

                let agent = self.load_agent(workspace_id, step.agent.as_deref()).await?;
                // A step may name its own engine, and so may the agent bound
                // to it; the workflow's default is what they fall back to.
                let step_engine_id = step
                    .engine
                    .clone()
                    .or_else(|| agent.as_ref().and_then(|a| a.engine.clone()))
                    .unwrap_or_else(|| engine_id.clone());
                let step_engine = self
                    .engine(&step_engine_id)
                    .ok_or_else(|| anyhow::anyhow!("unknown engine {step_engine_id}"))?;
                let model_id = workflow.resolve_model(
                    step,
                    agent.as_ref().map(|a| a.tier),
                    &self.tier_mapping().for_engine(&step_engine_id),
                );
                let prompt = aichip_shared::interpolate(&step.prompt, &outputs);

                let resolved = self.workflow_permission_mode(
                    &workflow,
                    agent.as_ref(),
                    full_auto_opt_in,
                    &shared.path,
                );
                let permission_mode = resolved.mode;

                // Nobody is at the keyboard at 3am. Parking no longer freezes
                // the queue — a parked run lends its slot back — but a
                // scheduled step that stops to ask would still burn tokens
                // getting to the question, wait out the whole attention
                // window, and then be cancelled unanswered. Refusing at
                // dispatch is the same outcome for free, and it says so.
                //
                // Manual runs still park, and that is now a real offer rather
                // than an assumption: someone chose to start them, the card
                // says plainly that it is waiting, and the hook can go and
                // tell them.
                if trigger == "schedule" && permission_mode == PermissionMode::Reviewed {
                    anyhow::bail!(
                        "{}",
                        if resolved.downgraded {
                            format!(
                                "step {step_id} asks for Don't-ask, which this project hasn't \
opted into, so it falls back to asking permission — and a scheduled run has nobody to ask. \
Turn on \"Don't ask\" for the project, or give the step Auto-edit."
                            )
                        } else {
                            format!(
                                "step {step_id} runs in Reviewed mode, which needs someone to \
approve each tool call, and a scheduled run has nobody to ask. Give it Auto-edit, or run \
this workflow manually."
                            )
                        }
                    );
                }

                // Resume the session of the step we depend on, so a
                // "continue" stage keeps the prior stage's context.
                let resume = if step.session == SessionMode::Continue {
                    step.needs
                        .first()
                        .and_then(|n| sessions.get(n))
                        .filter(|(engine, _)| engine == &step_engine_id)
                        .map(|(_, sid)| sid.clone())
                } else {
                    None
                };

                // Standing context, and both halves of *when* are load bearing.
                //
                // Only on a step that starts fresh. A `continue` step is the
                // same conversation as the one it follows and already has it;
                // appending again pays twice and puts a second "read this as
                // background" fence in one context, which is how a framing
                // stops being read as a framing.
                //
                // And strictly after `interpolate`, which splices the previous
                // steps' *model-generated* output into every `{{ … }}` in the
                // whole string. Augment first and that output can land inside
                // the brain's fence, where nothing has neutralised it — the
                // fence is scrubbed against the body as it stood when the
                // block was built, not against text spliced in afterwards.
                //
                // Brain only: `workflow::Step` has no `skill` field, and a
                // Skill is named, never inferred.
                let prompt = match resume {
                    None => standing.apply(&prompt),
                    Some(_) => prompt,
                };

                // Refuse a step this engine can't honour, before it runs.
                if let Err(reason) =
                    aichip_engines::vet(step_engine.as_ref(), permission_mode, resume.is_some())
                {
                    anyhow::bail!("step {step_id}: {reason}");
                }

                // Steps used to get no MCP wiring at all while still asking
                // for `permission_prompt_tool` — so a step that did stop to
                // ask pointed the CLI at a server that wasn't there, and the
                // agent's own connections were silently unavailable.
                let user_servers = crate::mcp_servers::for_agent(&self.db, agent.as_ref().map(|a| a.id))
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!(%run_id, %step_id, error=%e, "could not load step MCP servers");
                        vec![]
                    });
                let mcp = McpWiring {
                    aichip_url: self
                        .mcp_base_url
                        .as_ref()
                        .map(|b| format!("{b}/mcp/run/{run_id}")),
                    servers: user_servers.iter().map(|s| s.to_spec()).collect(),
                };
                // Same rule as task runs: an empty list means "no flag",
                // which allows everything, so only extend one that exists.
                let mut step_tools = agent
                    .as_ref()
                    .map(|a| a.allowed_tools.clone())
                    .unwrap_or_default();
                if !step_tools.is_empty() {
                    step_tools.extend(user_servers.iter().map(|s| s.tool_prefix()));
                }

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
                    .filter_map(|_| self.slots.sem().try_acquire_owned().ok())
                    .collect();
                let concurrency = 1 + extra.len();
                // Read once for the fan-out rather than per step: it is the
                // same setting for all of them, and the closure below is not
                // async.
                let tool_timeout_ms = self.mcp_tool_timeout_ms().await;

                let results = futures::stream::iter(plans.into_iter().map(
                    |(db_step_id, step_key, cwd)| {
                        let this = self.clone();
                        let engine = step_engine.clone();
                        let seq = seq.clone();
                        let tool_timeout_ms = tool_timeout_ms.clone();
                        let spec = RunSpec {
                            cwd,
                            prompt: prompt.clone(),
                            model_tier: agent.as_ref().map(|a| a.tier).unwrap_or_default(),
                            model_id: model_id.clone(),
                            effort: agent.as_ref().and_then(|a| a.effort),
                            resume_session_id: resume.clone(),
                            permission_mode,
                            allowed_tools: step_tools.clone(),
                                            append_system_prompt: agent
                                .as_ref()
                                .and_then(|a| (!a.system_prompt.is_empty()).then(|| a.system_prompt.clone())),
                            denied_tools: vec![],
                            mcp: mcp.clone(),
                            // Per step, not per run: concurrent steps would
                            // otherwise write the same scratch config path.
                            run_key: db_step_id.to_string(),
                            extra_read_dirs: vec![],
                            permission_prompt_tool: true,
                            extra_env: HashMap::from([
                                ("AICHIP_RUN_ID".to_string(), run_id.to_string()),
                                ("AICHIP_STEP".to_string(), step_key.clone()),
                                ("MCP_TOOL_TIMEOUT".to_string(), tool_timeout_ms.clone()),
                            ]),
                        };
                        async move {
                            let outcome = this
                                .stream_run(run_id, Some(db_step_id), &seq, engine, spec, CallerKind::WorkflowStep)
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
                    self.finish_step_row(db_step_id, &engine_id, &outcome).await?;
                    if outcome.status != RunStatus::Completed {
                        failure = Some(format!(
                            "step '{step_key}' {}: {}",
                            outcome.status.as_str(),
                            outcome.reason.clone().unwrap_or_default()
                        ));
                        break 'layers;
                    }
                    if let Some(sid) = outcome.session_id.clone() {
                        sessions
                            .entry(step_id.clone())
                            .or_insert((step_engine_id.clone(), sid));
                    }
                    step_outputs.push(outcome.output);
                }
                outputs.insert(step_id.clone(), step_outputs);
            }
        }

        if failure.as_deref() == Some("canceled") {
            self.finish(run_id, RunStatus::Canceled, None).await?;
            self.settle_task_for_run(task_id, RunStatus::Canceled).await?;
            return Ok(());
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

    /// Store the plan and stop, releasing the queue slot.
    ///
    /// Returning is the point: a run that sat here waiting would hold its
    /// concurrency permit for however long a person takes to read, and with a
    /// small default that is most of the queue. Approval re-queues it, and the
    /// phase decision then sends it to work.
    async fn park_for_approval(
        self: &Arc<Self>,
        run_id: Uuid,
        outcome: &StreamOutcome,
    ) -> anyhow::Result<()> {
        // A plan that never arrived is a failed run, not an empty approval
        // prompt — there is nothing for anyone to say yes to.
        if outcome.status != RunStatus::Completed || outcome.output.trim().is_empty() {
            let reason = outcome.reason.clone().unwrap_or_else(|| {
                "the planning pass produced no plan, so there is nothing to approve".to_string()
            });
            self.finish(run_id, RunStatus::Failed, Some(reason)).await?;
            return Ok(());
        }

        // Replace rather than accumulate: a revised plan supersedes the one
        // that was sent back, and two 'plan' rows would make `stored_plan`
        // return whichever the database felt like.
        sqlx::query("DELETE FROM steps WHERE run_id = $1 AND step_key = 'plan'")
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        sqlx::query(
            "INSERT INTO steps (run_id, step_key, status, session_id, session_engine,
                                output_text, started_at, finished_at)
             VALUES ($1, 'plan', 'completed', $2, $3, $4, now(), now())",
        )
        .bind(run_id)
        .bind(&outcome.session_id)
        .bind(sqlx::query_scalar::<_, String>("SELECT engine FROM runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&self.db.pool)
            .await?)
        .bind(&outcome.output)
        .execute(&self.db.pool)
        .await?;

        // The card stays in "running": it is not in review — there is no diff
        // — and certainly not done. The activity view already sorts
        // awaiting_approval to the top and lists it as a blocker.
        self.set_status(run_id, RunStatus::AwaitingApproval).await?;
        let ctx = crate::attention::Ctx {
            title: "aichip: a plan needs your review".to_string(),
            ..crate::attention::ctx_for_run(&self.db, run_id, None).await
        };
        crate::attention::fire(&self.db, crate::attention::Event::Plan, ctx).await;
        Ok(())
    }

    /// The plan this run has on file, if any.
    ///
    /// A `steps` row rather than a column on `runs`: it is a thing the agent
    /// produced, with a session behind it, and organizations already store
    /// their plans exactly this way.
    async fn stored_plan(&self, run_id: Uuid) -> anyhow::Result<Option<String>> {
        Ok(sqlx::query_scalar::<_, Option<String>>(
            "SELECT output_text FROM steps
             WHERE run_id = $1 AND step_key = 'plan' AND status = 'completed'",
        )
        .bind(run_id)
        .fetch_optional(&self.db.pool)
        .await?
        .flatten()
        .filter(|t| !t.trim().is_empty()))
    }

    /// Outstanding feedback on a rejected plan, consumed as it is read.
    ///
    /// Cleared here rather than by the route, so a revise request survives a
    /// crash between asking and dispatching but can never be replayed into a
    /// second pass it wasn't meant for.
    async fn plan_revision_note(&self, run_id: Uuid) -> anyhow::Result<Option<String>> {
        // A CTE, because `RETURNING` hands back the *new* row — reading the
        // column it was just cleared to would always be null.
        Ok(sqlx::query_scalar::<_, Option<String>>(
            "WITH prev AS (SELECT plan_note FROM runs WHERE id = $1)
             UPDATE runs SET plan_note = NULL WHERE id = $1
             RETURNING (SELECT plan_note FROM prev)",
        )
        .bind(run_id)
        .fetch_optional(&self.db.pool)
        .await?
        .flatten()
        .filter(|n| !n.trim().is_empty()))
    }

    /// Did a person rewrite the plan, rather than approve what was proposed?
    async fn plan_was_edited(&self, run_id: Uuid) -> anyhow::Result<bool> {
        Ok(
            sqlx::query_scalar::<_, bool>("SELECT plan_edited FROM runs WHERE id = $1")
                .bind(run_id)
                .fetch_optional(&self.db.pool)
                .await?
                .unwrap_or(false),
        )
    }

    /// The session the planning pass ran in, so the work pass can pick up the
    /// context it built while reading the code rather than re-reading it all.
    async fn plan_session(&self, run_id: Uuid) -> anyhow::Result<Option<(String, String)>> {
        let row = sqlx::query(
            "SELECT session_id, session_engine FROM steps
             WHERE run_id = $1 AND step_key = 'plan'",
        )
        .bind(run_id)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.and_then(|r| {
            match (
                r.get::<Option<String>, _>("session_id"),
                r.get::<Option<String>, _>("session_engine"),
            ) {
                (Some(sid), Some(engine)) => Some((sid, engine)),
                _ => None,
            }
        }))
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
        engine_id: &str,
        outcome: &StreamOutcome,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE steps SET status=$1, session_id=$2, session_engine=$5, output_text=$3,
             finished_at=now() WHERE id=$4",
        )
        .bind(outcome.status.as_str())
        .bind(&outcome.session_id)
        .bind(&outcome.output)
        .bind(step_id)
        .bind(engine_id)
        .execute(&self.db.pool)
        .await?;
        Ok(())
    }

    /// What is known about a card before its run starts, for the auto tier.
    ///
    /// One round trip, and every signal is structural — how much was attached,
    /// how much was written, what happened last time. Deliberately *not* the
    /// project's historical cost: that measures the model previously used, so
    /// routing on it would close a loop where cheap runs keep justifying the
    /// cheap tier. It belongs on the spend page as context, not in here.
    ///
    /// Best-effort: a signal query that fails must not fail the run, so a
    /// failure yields empty signals, which classify to Medium — the same tier
    /// the card would have had before any of this existed.
    async fn tier_signals(
        &self,
        run_id: Uuid,
        task_id: Uuid,
        run: &sqlx::postgres::PgRow,
    ) -> aichip_shared::TierSignals {
        let title: String = run.get("title");
        let prompt: String = run.get("prompt");
        let mut signals = aichip_shared::TierSignals {
            brief_chars: title.chars().count() + prompt.chars().count(),
            replans: run.try_get("replans").unwrap_or(0),
            ..Default::default()
        };

        let row = sqlx::query(
            "SELECT (SELECT count(*) FROM attachments   WHERE task_id = $1) AS attachments,
                    (SELECT count(*) FROM task_articles WHERE task_id = $1) AS articles,
                    (SELECT status        FROM runs WHERE task_id = $1 AND id <> $2
                      ORDER BY created_at DESC LIMIT 1) AS prior_status,
                    (SELECT tier_resolved FROM runs WHERE task_id = $1 AND id <> $2
                       AND tier_resolved IS NOT NULL
                      ORDER BY created_at DESC LIMIT 1) AS prior_tier",
        )
        .bind(task_id)
        .bind(run_id)
        .fetch_optional(&self.db.pool)
        .await;

        match row {
            Ok(Some(r)) => {
                signals.attachments = r.get::<i64, _>("attachments").max(0) as usize;
                signals.kb_articles = r.get::<i64, _>("articles").max(0) as usize;
                signals.prior_failed = matches!(
                    r.get::<Option<String>, _>("prior_status").as_deref(),
                    Some("failed")
                );
                signals.prior_tier = r
                    .get::<Option<String>, _>("prior_tier")
                    .and_then(|t| TierChoice::parse(&t))
                    .and_then(TierChoice::fixed);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(%run_id, error = %e, "auto tier signals unavailable"),
        }
        signals
    }

    /// Apply one step's token delta to the run, and to the step row when there
    /// is one.
    ///
    /// Additive rather than absolute so a fan-out's parallel steps can each
    /// write their own share of the same run without reading first — and
    /// therefore without clobbering each other.
    ///
    /// `provisional` marks figures no final engine message ever reconciled. It
    /// is OR-ed in, never cleared: one estimated step makes the run's total an
    /// estimate, and a later exact step does not make the earlier guess true.
    async fn flush_usage(
        &self,
        run_id: Uuid,
        step_id: Option<Uuid>,
        delta: UsageDelta,
        provisional: bool,
    ) -> anyhow::Result<()> {
        if delta.is_zero() && !provisional {
            return Ok(());
        }
        sqlx::query(
            "UPDATE runs SET input_tokens          = input_tokens          + $2,
                             output_tokens         = output_tokens         + $3,
                             cache_read_tokens     = cache_read_tokens     + $4,
                             cache_creation_tokens = cache_creation_tokens + $5,
                             tokens_provisional    = tokens_provisional OR $6
             WHERE id=$1",
        )
        .bind(run_id)
        .bind(delta.input)
        .bind(delta.output)
        .bind(delta.cache_read)
        .bind(delta.cache_creation)
        .bind(provisional)
        .execute(&self.db.pool)
        .await?;

        if let Some(step_id) = step_id {
            sqlx::query(
                "UPDATE steps SET input_tokens          = input_tokens          + $2,
                                  output_tokens         = output_tokens         + $3,
                                  cache_read_tokens     = cache_read_tokens     + $4,
                                  cache_creation_tokens = cache_creation_tokens + $5
                 WHERE id=$1",
            )
            .bind(step_id)
            .bind(delta.input)
            .bind(delta.output)
            .bind(delta.cache_read)
            .bind(delta.cache_creation)
            .execute(&self.db.pool)
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn load_agent(
        &self,
        workspace_id: Uuid,
        name: Option<&str>,
    ) -> anyhow::Result<Option<BoundAgent>> {
        let Some(name) = name else { return Ok(None) };
        let row = sqlx::query(
            "SELECT id, system_prompt, model_tier, effort, allowed_tools, permission_preset,
                    engine
             FROM agents WHERE workspace_id=$1 AND name=$2",
        )
        .bind(workspace_id)
        .bind(name)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.map(|r| BoundAgent {
            id: r.get("id"),
            system_prompt: r.get("system_prompt"),
            // An agent cannot be set to `auto` from the UI yet; if one ever is,
            // it resolves to Medium here rather than falling through an
            // unwrap_or_default that would read as a deliberate choice.
            tier: TierChoice::parse(&r.get::<String, _>("model_tier"))
                .and_then(TierChoice::fixed)
                .unwrap_or_default(),
            effort: r
                .get::<Option<String>, _>("effort")
                .and_then(|e| ReasoningEffort::parse(&e)),
            allowed_tools: r.get("allowed_tools"),
            permission_preset: r.get("permission_preset"),
            engine: r.get("engine"),
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
    ) -> StepPermission {
        let spec = agent
            .and_then(|a| a.permission_preset.clone())
            .or_else(|| workflow.defaults.permission_mode.clone());
        let asked = spec
            .and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok())
            .unwrap_or(PermissionMode::AutoEdit);
        resolve_step_permission(
            asked,
            full_auto_opt_in && self.worktrees.manages(cwd),
        )
    }

    /// Shared streaming loop: spawn the engine, persist+publish every event,
    /// handle cancel / rate-limit / terminal transitions.
    ///
    /// `step_id` tags events for pipeline steps. `caller` answers both of the
    /// questions the ending depends on — who owns the run's final status, and
    /// whether a rate limit can be waited out or has to be reported — which
    /// used to be one `bool` answering only the first.
    pub(crate) async fn stream_run(
        self: &Arc<Self>,
        run_id: Uuid,
        step_id: Option<Uuid>,
        seq: &SeqAlloc,
        engine: Arc<dyn Engine>,
        spec: RunSpec,
        caller: CallerKind,
    ) -> anyhow::Result<StreamOutcome> {
        let finalize = caller.finalizes();
        let mut proc = engine.start(spec)?;
        self.set_status(run_id, RunStatus::Running).await?;

        // Registered under the run, with a per-step slot so a fan-out's
        // steps don't clobber each other. A cancel that arrived while this
        // step was starting is honoured immediately rather than lost.
        let step_key = step_id.unwrap_or(run_id);
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        {
            let mut cancels = self.cancels.lock().unwrap();
            let state = cancels.entry(run_id).or_default();
            let already = state.requested;
            state.steps.insert(step_key, cancel_tx);
            if already {
                // Asked to stop while this step was starting: fire it now
                // rather than letting the request fall through the gap.
                if let Some(tx) = state.steps.remove(&step_key) {
                    let _ = tx.send(());
                }
            }
        }

        let mut outcome: Option<(RunStatus, Option<String>)> = None;
        let mut text_parts: Vec<String> = vec![];
        let mut result_text = String::new();
        let mut session_id: Option<String> = None;
        // Both adapters report usage twice — per message as they go, and
        // authoritatively at the end. The tally reconciles the two so the row
        // is neither double-counted nor left at zero when no final message
        // arrives; see `usage_tally` for why that is not just a sum.
        let mut tally = UsageTally::default();
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
                                sqlx::query(
                                    "UPDATE runs SET session_id=$1, session_engine=$2 WHERE id=$3",
                                )
                                    .bind(sid).bind(engine.id()).bind(run_id)
                                    .execute(&self.db.pool).await?;
                            }
                        }
                        AichipEvent::RunCompleted { session_id: sid, cost_usd, usage, result_text: rt } => {
                            session_id = Some(sid.clone());
                            result_text = rt.clone();
                            // The engine's own figures replace whatever the
                            // mid-run telemetry estimated. Tokens are written
                            // once after the loop, so this only adopts them.
                            tally.adopt(usage);
                            // Costs accumulate: a workflow run has many steps.
                            sqlx::query(
                                "UPDATE runs SET session_id=$1, session_engine=$4,
                                 cost_usd = COALESCE(cost_usd, 0) + COALESCE($2, 0)
                                 WHERE id=$3")
                                .bind(sid)
                                .bind(cost_usd)
                                .bind(run_id)
                                .bind(engine.id())
                                .execute(&self.db.pool).await?;
                            if let Some(step_id) = step_id {
                                sqlx::query("UPDATE steps SET cost_usd = COALESCE(cost_usd, 0) + COALESCE($2, 0) WHERE id=$1")
                                    .bind(step_id)
                                    .bind(cost_usd)
                                    .execute(&self.db.pool).await?;
                            }
                            outcome = Some((RunStatus::Completed, None));
                        }
                        AichipEvent::UsageUpdated { usage } => {
                            // Only an estimate until a final message arrives —
                            // but the only figures a cancelled run will ever
                            // have, which is why they are kept at all.
                            tally.observe(usage);
                        }
                        AichipEvent::RunFailed { reason } => {
                            outcome = Some((RunStatus::Failed, Some(reason.clone())));
                        }
                        AichipEvent::UsageStatus {
                            limit_type,
                            status,
                            resets_at,
                            using_overage,
                        } => {
                            // Telemetry, never an outcome — it must not touch
                            // `outcome`, or a healthy ping would end the run.
                            if let Err(e) = crate::usage::record(
                                &self.db,
                                engine.id(),
                                limit_type,
                                status,
                                *resets_at,
                                *using_overage,
                            )
                            .await
                            {
                                tracing::warn!(error=%e, "could not record plan usage");
                            }
                        }
                        AichipEvent::RateLimited { reset_at, message } => {
                            // Held or failed, never both, and never a queue
                            // row without the status that explains it — see
                            // `CallerKind::on_rate_limit`.
                            outcome = Some(match caller.on_rate_limit() {
                                OnRateLimit::Hold => {
                                    self.hold_rate_limited(run_id, *reset_at).await?;
                                    (RunStatus::RateLimited, Some(message.clone()))
                                }
                                OnRateLimit::Fail => (
                                    RunStatus::Failed,
                                    Some(format!("{message} (this run can't be held and resumed, so it stopped here)")),
                                ),
                            });
                            let ctx = crate::attention::Ctx {
                                title: "aichip: rate limited".to_string(),
                                ..crate::attention::ctx_for_run(&self.db, run_id, None).await
                            };
                            crate::attention::fire(
                                &self.db,
                                crate::attention::Event::RateLimited,
                                ctx,
                            )
                            .await;
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(state) = self.cancels.lock().unwrap().get_mut(&run_id) {
            state.steps.remove(&step_key);
        }

        // The step's tokens, written exactly once, whichever way it ended. A
        // run that was cancelled or whose engine died never sent a final
        // message, and used to record zero — so an interrupted session showed
        // as free, and the daily budget under-counted it to match.
        let provisional = tally.is_provisional();
        let delta = tally.take_delta();
        if let Err(e) = self.flush_usage(run_id, step_id, delta, provisional).await {
            // Never fail a run over its own bookkeeping.
            tracing::warn!(%run_id, error = %e, "could not record token usage");
        }

        let (status, reason) =
            outcome.unwrap_or((RunStatus::Failed, Some("event stream ended unexpectedly".into())));
        // A held run is already `rate_limited` with a queue row behind it, put
        // there together by `hold_rate_limited`; finishing it here would strand
        // that row under a terminal status, which is the leak this slice is
        // about. Every other ending is the caller's to record, if it owns one.
        if finalize && status != RunStatus::RateLimited {
            self.finish(run_id, status, reason.clone()).await?;
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

    /// Put a rate-limited run back on the queue behind a backoff, and mark it
    /// held — the two halves written together, in the one place either is
    /// written.
    ///
    /// They used to be separate: the queue row went in here, unconditionally,
    /// while the status was set by `stream_run` only when it was finalizing.
    /// Every caller that did not finalize therefore left a queue row under a
    /// run that never said it was waiting, and `finish` did not clean up after
    /// it. `claim_next` popped it five minutes later and re-ran the whole
    /// pipeline, charged in full.
    ///
    /// The counter climbs first so the *first* hold reads attempt 0. It lives
    /// on `runs` because `claim_next` is a `DELETE … RETURNING run_id` — a
    /// column on `queue` would have to be threaded through five signatures to
    /// reach this one number — and because a resumed or retried run is a new
    /// row, so it resets with no reset logic.
    async fn hold_rate_limited(
        &self,
        run_id: Uuid,
        reset_at: Option<DateTime<Utc>>,
    ) -> anyhow::Result<()> {
        let holds: i32 = sqlx::query_scalar(
            "UPDATE runs SET status='rate_limited', rate_limit_attempts = rate_limit_attempts + 1
             WHERE id=$1 RETURNING rate_limit_attempts",
        )
        .bind(run_id)
        .fetch_one(&self.db.pool)
        .await?;
        let not_before = rate_limit_backoff(crate::queue::attempt_index(holds), reset_at);
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
        // `finish` means ended. `rate_limited` is a *held* run — it is coming
        // back — and writing it through here would set `finished_at` on a run
        // that has not finished and delete the queue row that brings it back.
        // Debug-assert rather than return an error, because the callers that
        // could get this wrong are inside this file and this is a programming
        // mistake, not a runtime condition.
        debug_assert_ne!(status, RunStatus::RateLimited, "a held run has not finished");
        self.forget_cancel(run_id);
        let mut tx = self.db.pool.begin().await?;
        // `COALESCE`, because a cancel carries `reason: None` and the broker
        // may already have written the true one — "nobody answered the request
        // to allow Bash". Safe only because `unpark` clears the column, so a
        // run that parked and then finished cleanly reports nothing.
        sqlx::query(
            "UPDATE runs SET status=$1, error_reason=COALESCE($2, error_reason),
             finished_at=now() WHERE id=$3",
        )
            .bind(status.as_str())
            .bind(reason)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        // Nothing waiting to be dispatched can outlive the run it belongs to.
        // A run reaches here from a cancel, a crash in `execute`, a failed
        // dispatch or a plain ending, and any of those can happen while a
        // queue row exists — a held run someone cancelled, most obviously.
        // Left behind, that row is re-claimed later and `execute` runs the
        // whole thing again on a row that reads `failed`.
        sqlx::query("DELETE FROM queue WHERE run_id=$1")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        // A cancel mid-step, or a failure that skipped the per-step bookkeeping,
        // leaves step rows non-terminal under a terminal run. Normally a no-op.
        settle_steps(&mut tx, &[run_id], status).await?;

        // A run that ended badly takes its card off In Progress with it.
        // Without this the card sat under In Progress for good — nothing
        // working on it, no pulse, and no way out but to drag it back by hand.
        // It lives here rather than at the end of `run_task` because the
        // endings that need it most never reach `run_task`: a dispatch that
        // bailed on an unknown engine failed before the function was entered.
        //
        // Review, because that already means "a person needs to look at this"
        // and is where the badge and the Retry button are — the same rule
        // `org::epic::COLUMN_FOR_STEP` states for a step that ended badly.
        //
        // Two guards, both load bearing. Only a card actually *on* In Progress
        // moves, so a failure never drags one out of Backlog or back from Done.
        // And only when nothing else is still working on it: a bake-off runs
        // several runs against one card, and the first variant to fail must not
        // send it to review while the others are still going.
        if status != RunStatus::Completed {
            sqlx::query(
                "UPDATE tasks SET board_column = 'review'
                  WHERE id = (SELECT task_id FROM runs WHERE id = $1)
                    AND board_column = 'running'
                    AND NOT EXISTS (
                          SELECT 1 FROM runs other
                           WHERE other.task_id = tasks.id
                             AND other.status NOT IN ('completed', 'failed', 'canceled'))",
            )
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        // Every ending comes through here — completion, cancellation, a crash in
        // `execute`, a planning failure — which is why the epic mirror hangs off
        // `finish` rather than off the end of `work_phase`. A no-op unless the
        // run has steps with cards.
        if let Err(e) = crate::runs::org::epic::mirror_run(&self.db, run_id).await {
            tracing::warn!(%run_id, error=%e, "could not update this run's sub-task cards");
        }
        // Same reasoning for routines: `finish` is the one door every ending
        // leaves through, and a routine's whole point is running while nobody
        // watches. A no-op unless the run was a routine firing.
        crate::routines::announce_finished(&self.db, run_id, status).await;
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

/// Drag step rows to a terminal state along with the run that owned them.
///
/// The UI derives "who is working right now" from step status, so a step left
/// at 'running' under a failed run reads as a live teammate forever. Two
/// buckets, because they are not the same fact: a step that was mid-flight
/// really did fail (or was canceled with the run), while a step still queued
/// was never opened — calling that one 'failed' would paint its assignee as
/// blocked on work they never started.
pub(crate) async fn settle_steps(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    run_ids: &[Uuid],
    run_status: RunStatus,
) -> anyhow::Result<()> {
    if run_ids.is_empty() {
        return Ok(());
    }
    let interrupted = if run_status == RunStatus::Canceled {
        RunStatus::Canceled
    } else {
        RunStatus::Failed
    };
    sqlx::query(
        "UPDATE steps SET status=$1, finished_at=now()
         WHERE run_id = ANY($2)
           AND status IN ('starting','running','waiting_permission','rate_limited')",
    )
    .bind(interrupted.as_str())
    .bind(run_ids)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE steps SET status='skipped', finished_at=now()
         WHERE run_id = ANY($1) AND status='queued'",
    )
    .bind(run_ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
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
/// Turn a review note into a brief for the agent.
///
/// Pure so the shape can be tested without a database. Three things have to
/// survive into the prompt: where the note points, what the code looked like
/// when it was written, and a scope limit — a review note is not licence to
/// keep working on the task.
fn review_fix_prompt(
    task_prompt: &str,
    file_path: Option<&str>,
    line: Option<i32>,
    hunk: Option<&str>,
    note: &str,
) -> String {
    let mut prompt = String::from("You are acting on review feedback for work you already did.\n\n");
    match (file_path, line) {
        (Some(path), Some(line)) => {
            prompt.push_str(&format!("The reviewer commented on {path}, around line {line}.\n"))
        }
        (Some(path), None) => prompt.push_str(&format!("The reviewer commented on {path}.\n")),
        _ => prompt.push_str("The reviewer commented on the change as a whole.\n"),
    }
    if let Some(hunk) = hunk.filter(|h| !h.trim().is_empty()) {
        // Line numbers drift the moment you edit; the snapshot is what
        // actually identifies the code being talked about.
        prompt.push_str(&format!(
            "\nThe code as it stood when they wrote the note:\n```diff\n{}\n```\n",
            clip_chars(hunk, 2000),
        ));
    }
    prompt.push_str(&format!("\nTheir note:\n{note}\n"));
    prompt.push_str(&format!(
        "\nFor context, the original task was:\n{}\n",
        clip_chars(task_prompt, 800),
    ));
    prompt.push_str(
        "\nMake exactly this change and stop. Do not refactor beyond it, do not \
         revisit other review notes, and do not continue the original task. If \
         the note is a question rather than a request, answer it without editing \
         anything. Finish with one short line saying what you changed.",
    );
    prompt
}

/// Truncate on a character boundary, marking that something was dropped.
pub(crate) fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "\n…"
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
    use super::*;

    /// Every caller, so a seventh cannot be added without answering.
    const CALLERS: [CallerKind; 8] = [
        CallerKind::TaskWork,
        CallerKind::TaskPlanning,
        CallerKind::WorkflowStep,
        CallerKind::OrgMember,
        CallerKind::Chat,
        CallerKind::CommentReply,
        CallerKind::KbGeneration,
        CallerKind::Research,
    ];

    /// The invariant the leak violated: a run only goes back on the queue if
    /// something will pick it up again unchanged.
    ///
    /// Holding is the interesting half. The two that are refused are refused
    /// because re-dispatching them costs money twice — a workflow re-runs
    /// every step it already paid for, and an org run's batch loop has already
    /// moved on and will write a terminal status over the hold.
    #[test]
    fn only_a_run_that_can_be_re_dispatched_unchanged_is_held() {
        for caller in CALLERS {
            let expected = match caller {
                CallerKind::WorkflowStep | CallerKind::OrgMember => OnRateLimit::Fail,
                _ => OnRateLimit::Hold,
            };
            assert_eq!(caller.on_rate_limit(), expected, "{caller:?}");
        }
    }

    /// The half of `CallerKind` that used to be a bare `bool`, pinned so the
    /// consolidation cannot have changed anyone's ending by accident.
    #[test]
    fn only_a_whole_run_finalizes() {
        for caller in CALLERS {
            let expected = !matches!(
                caller,
                CallerKind::TaskPlanning | CallerKind::WorkflowStep | CallerKind::OrgMember
            );
            assert_eq!(caller.finalizes(), expected, "{caller:?}");
        }
    }

    /// Why a resumed run must be created with `plan_approval = false`.
    ///
    /// Carrying it forward would send the resumed run back into the planning
    /// half — mutating tools denied, another plan written, nothing done — on a
    /// session that has already been working. Asserted here rather than
    /// trusted, because the resume path cannot see this decision.
    #[test]
    fn a_plan_first_run_with_no_stored_plan_plans_again() {
        use crate::runs::task_plan::{decide, Phase, PlanStep};
        assert_eq!(decide(true, PlanStep::Missing, true), Phase::Plan);
        assert_eq!(decide(false, PlanStep::Missing, true), Phase::Work { plan: None });
    }

    /// The chat assistant must never be handed a mode its engine cannot honour.
    ///
    /// This is the bug this test was written for: chat passed a flat `Reviewed`,
    /// and an engine that cannot pause to ask answers that by rejecting every
    /// tool call. The assistant went quiet — no repository access, no task
    /// tools — and asked the user what they were working on, which reads as
    /// "it can't see my project" rather than "it was refused".
    #[test]
    fn chat_never_asks_an_engine_to_review_when_it_cannot() {
        for engine in [
            &aichip_engines::claude::ClaudeEngine::default() as &dyn aichip_engines::Engine,
            &aichip_engines::opencode::OpenCodeEngine::default() as &dyn aichip_engines::Engine,
        ] {
            let mode = chat_permission_mode(engine);
            assert_ne!(mode, PermissionMode::Reviewed, "{}", engine.label());
            assert!(
                aichip_engines::vet(engine, mode, false).is_ok(),
                "{} cannot honour {mode:?}",
                engine.label()
            );
        }
    }

    /// Approve-everything is only safe because "everything" is a short list.
    #[test]
    fn the_chat_assistant_cannot_reach_a_tool_that_writes() {
        for tool in ["Edit", "Write", "MultiEdit", "NotebookEdit", "Bash"] {
            assert!(
                CHAT_DENIED_TOOLS.contains(&tool),
                "{tool} must be denied by name — an allow-list only pre-approves, \
                 it does not forbid, and chat runs in the real checkout"
            );
            assert!(!CHAT_ALLOWED_TOOLS.contains(&tool));
        }
    }

    /// The gate exists so a workflow can't grant itself more freedom than the
    /// project allows. Down, never up.
    #[test]
    fn full_auto_is_cut_to_reviewed_when_the_project_has_not_opted_in() {
        let r = resolve_step_permission(PermissionMode::FullAuto, false);
        assert_eq!(r.mode, PermissionMode::Reviewed);
        assert!(r.downgraded, "the caller has to be able to tell this apart");

        let r = resolve_step_permission(PermissionMode::FullAuto, true);
        assert_eq!(r.mode, PermissionMode::FullAuto);
        assert!(!r.downgraded);
    }

    /// The inverse would be a privilege escalation performed on the user's
    /// behalf, which is exactly what the compliance rules forbid.
    #[test]
    fn nothing_is_ever_raised_by_the_gate() {
        for asked in [
            PermissionMode::Reviewed,
            PermissionMode::AutoEdit,
            PermissionMode::FullAuto,
        ] {
            for gate in [true, false] {
                let got = resolve_step_permission(asked, gate).mode;
                assert!(
                    got == asked || (asked == PermissionMode::FullAuto
                        && got == PermissionMode::Reviewed),
                    "{asked:?} with gate={gate} became {got:?}"
                );
            }
        }
    }

    /// A step that merely *asks* for Reviewed is a different story from one
    /// that was cut down to it — the scheduled-run refusal says so, and a
    /// person can only act on the right one.
    #[test]
    fn a_step_that_asked_for_reviewed_is_not_reported_as_downgraded() {
        let r = resolve_step_permission(PermissionMode::Reviewed, false);
        assert_eq!(r.mode, PermissionMode::Reviewed);
        assert!(!r.downgraded);
    }

    #[test]
    fn a_review_note_becomes_a_scoped_brief() {
        let prompt = review_fix_prompt(
            "Build the leads finder",
            Some("backend/app/routes.py"),
            Some(42),
            Some("- return None\n+ return leads"),
            "This swallows the error; raise instead.",
        );
        assert!(prompt.contains("backend/app/routes.py"));
        assert!(prompt.contains("line 42"));
        assert!(prompt.contains("return leads"), "the hunk grounds the note");
        assert!(prompt.contains("raise instead"));
        // The scope limit is the point: a review note must not restart the task.
        assert!(prompt.contains("do not continue the original task"));
    }

    #[test]
    fn a_note_without_an_anchor_still_works() {
        // Card-level review feedback has no file or line.
        let prompt = review_fix_prompt("Do the thing", None, None, None, "Rename the module.");
        assert!(prompt.contains("the change as a whole"));
        assert!(prompt.contains("Rename the module."));
        assert!(!prompt.contains("```diff"), "no hunk, no empty code fence");
    }

    #[test]
    fn a_huge_hunk_cannot_crowd_out_the_note() {
        let prompt = review_fix_prompt(
            &"task ".repeat(1000),
            Some("a.rs"),
            Some(1),
            Some(&"x".repeat(50_000)),
            "Fix it.",
        );
        assert!(prompt.chars().count() < 4000);
        assert!(prompt.contains("Fix it."));
    }

    /// Clipping by chars, not bytes — a a multi-byte boundary would panic.
    #[test]
    fn clipping_respects_character_boundaries() {
        assert_eq!(clip_chars("héllo wörld", 5), "héllo\n…");
        assert_eq!(clip_chars("short", 99), "short");
    }

    /// The bug this guards: cancel channels were keyed by step id while
    /// `cancel()` looked them up by run id, so cancelling any multi-step
    /// run silently did nothing.
    #[test]
    fn cancelling_reaches_every_live_step_of_a_run() {
        let cancels: Mutex<HashMap<Uuid, CancelState>> = Mutex::new(HashMap::new());
        let run = Uuid::new_v4();
        let mut receivers = vec![];

        // Three steps of one run register, as a fan-out would.
        for _ in 0..3 {
            let (tx, rx) = oneshot::channel();
            cancels
                .lock()
                .unwrap()
                .entry(run)
                .or_default()
                .steps
                .insert(Uuid::new_v4(), tx);
            receivers.push(rx);
        }

        // What cancel() does, by run id.
        let mut state = cancels.lock().unwrap();
        let entry = state.entry(run).or_default();
        entry.requested = true;
        for (_, tx) in std::mem::take(&mut entry.steps) {
            let _ = tx.send(());
        }
        drop(state);

        for mut rx in receivers {
            assert!(rx.try_recv().is_ok(), "every live step must be signalled");
        }
        assert!(cancels.lock().unwrap()[&run].requested, "intent outlives the steps");
    }

    /// A cancel arriving between steps must not be lost: the flag is what a
    /// multi-step run checks before starting the next assignment.
    #[test]
    fn cancel_intent_survives_when_no_step_is_live() {
        let cancels: Mutex<HashMap<Uuid, CancelState>> = Mutex::new(HashMap::new());
        let run = Uuid::new_v4();

        cancels.lock().unwrap().entry(run).or_default().requested = true;

        // The next step to start sees the request already standing.
        let requested = cancels.lock().unwrap()[&run].requested;
        assert!(requested);
    }

    use super::slugify;

    #[test]
    fn slugify_is_branch_safe() {
        assert_eq!(slugify("Fix: the (weird) bug!!"), "fix-the-weird-bug");
        assert!(slugify(&"x".repeat(100)).len() <= 40);
    }
}
