//! Organizations: a team with a manager.
//!
//! The manager reads the goal and the roster, splits the work into briefed
//! assignments, and hands them to specialists. Workers can talk to the team
//! and escalate questions back to the manager mid-task. The manager reviews
//! at the end. Every exchange lands in `org_messages`, which is what the
//! user watches.
//!
//! Assignments run one at a time in a shared worktree. Real teammates
//! working one codebase serialize too — and it means the run produces a
//! single coherent diff to review rather than N branches to merge.

use aichip_engines::RunSpec;
use aichip_shared::{ModelTier, PermissionMode, RunStatus};
use serde::Deserialize;
use sqlx::Row;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use super::orchestrator::{next_seq, slugify, write_mcp_config, Orchestrator, SeqAlloc};
use super::utility::extract_json;

/// Tools the whole org shares for talking to each other.
pub const ORG_TOOLS: &[&str] = &[
    "mcp__aichip__post_message",
    "mcp__aichip__read_messages",
    "mcp__aichip__ask_manager",
];

/// Planning is read-only: the manager inspects and delegates, never edits.
const MANAGER_TOOLS: &[&str] = &["Read", "Grep", "Glob", "mcp__aichip__post_message"];

/// Workers get the usual coding surface plus the org tools. Listed
/// explicitly because org runs don't use the interactive permission proxy.
const WORKER_TOOLS: &[&str] = &[
    "Read", "Grep", "Glob", "Edit", "Write", "MultiEdit", "NotebookEdit", "Bash", "TodoWrite",
];

/// A worker may escalate this many times before it has to make a call.
pub const MAX_CONSULTS_PER_STEP: i64 = 3;

#[derive(Debug, Deserialize)]
struct Plan {
    #[serde(default)]
    summary: String,
    tasks: Vec<PlannedTask>,
}

#[derive(Debug, Deserialize)]
struct PlannedTask {
    key: String,
    title: String,
    brief: String,
    assignee: String,
    #[serde(default)]
    depends_on: Vec<String>,
}

pub struct Member {
    pub name: String,
    pub title: String,
    pub description: String,
}

impl Orchestrator {
    /// Queue an organization run for a goal.
    pub async fn enqueue_org_run(
        &self,
        team_id: Uuid,
        project_id: Uuid,
        goal: &str,
    ) -> anyhow::Result<Uuid> {
        let row = sqlx::query(
            "INSERT INTO runs (team_id, project_id, goal, status, trigger, engine)
             VALUES ($1, $2, $3, 'queued', 'org', 'claude-code') RETURNING id",
        )
        .bind(team_id)
        .bind(project_id)
        .bind(goal)
        .fetch_one(&self.db.pool)
        .await?;
        let run_id: Uuid = row.get("id");
        sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, 15)")
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        Ok(run_id)
    }

    /// `team_id` is already on the run row; the dispatcher passes it in only
    /// so the match arm reads clearly.
    /// A board task assigned to a team. Organizations get an org run;
    /// pipeline/debate/swarm teams are translated to a workflow and run
    /// through the pipeline executor. Either way the run carries the
    /// `task_id`, so it works in the task's worktree and lands on review.
    pub(crate) async fn enqueue_task_for_team(
        &self,
        task_id: Uuid,
        team_id: Uuid,
    ) -> anyhow::Result<Uuid> {
        let row = sqlx::query(
            "SELECT t.prompt, t.title, t.project_id, tm.name AS team_name, tm.pattern, tm.definition
             FROM tasks t JOIN teams tm ON tm.id = t.team_id WHERE t.id = $1",
        )
        .bind(task_id)
        .fetch_one(&self.db.pool)
        .await?;

        let project_id: Uuid = row.get("project_id");
        let goal: String = row.get("prompt");
        let pattern: String = row.get("pattern");

        if pattern == "org" {
            let run = sqlx::query(
                "INSERT INTO runs (task_id, team_id, project_id, goal, status, trigger, engine)
                 VALUES ($1,$2,$3,$4,'queued','task','claude-code') RETURNING id",
            )
            .bind(task_id)
            .bind(team_id)
            .bind(project_id)
            .bind(&goal)
            .fetch_one(&self.db.pool)
            .await?;
            let run_id: Uuid = run.get("id");
            sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, 12)")
                .bind(run_id)
                .execute(&self.db.pool)
                .await?;
            return Ok(run_id);
        }

        // Non-org patterns become a workflow named after the task, so
        // re-running the task overwrites its workflow rather than piling up.
        let definition: serde_json::Value = row.get("definition");
        let names = self.member_names(&definition).await?;
        if names.is_empty() {
            anyhow::bail!("this team has no members to assign work to");
        }
        let team_name: String = row.get("team_name");
        let title: String = row.get("title");
        let mut workflow =
            aichip_shared::workflow::from_team(&team_name, &pattern, &names, &goal);
        workflow.name = format!("{team_name} · {title}");
        workflow.validate()?;
        let yaml = serde_yaml::to_string(&workflow)?;

        let wf = sqlx::query(
            "INSERT INTO workflows (project_id, name, description, kind, source_yaml)
             VALUES ($1,$2,$3,'team',$4)
             ON CONFLICT (project_id, name) DO UPDATE SET source_yaml = EXCLUDED.source_yaml
             RETURNING id",
        )
        .bind(project_id)
        .bind(&workflow.name)
        .bind(&workflow.description)
        .bind(&yaml)
        .fetch_one(&self.db.pool)
        .await?;
        let workflow_id: Uuid = wf.get("id");

        let run = sqlx::query(
            "INSERT INTO runs (task_id, workflow_id, status, trigger, engine)
             VALUES ($1,$2,'queued','task','claude-code') RETURNING id",
        )
        .bind(task_id)
        .bind(workflow_id)
        .fetch_one(&self.db.pool)
        .await?;
        let run_id: Uuid = run.get("id");
        sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, 12)")
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
        Ok(run_id)
    }

    /// Member agent names, in the order the team author arranged them.
    async fn member_names(&self, definition: &serde_json::Value) -> anyhow::Result<Vec<String>> {
        let mut names = vec![];
        for entry in definition
            .get("members")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            let Some(agent_id) = entry
                .get("agent_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                continue;
            };
            if let Some(r) = sqlx::query("SELECT name FROM agents WHERE id = $1")
                .bind(agent_id)
                .fetch_optional(&self.db.pool)
                .await?
            {
                names.push(r.get::<String, _>("name"));
            }
        }
        Ok(names)
    }

    pub(crate) async fn execute_org_run(
        self: &Arc<Self>,
        run_id: Uuid,
        _team_id: Uuid,
    ) -> anyhow::Result<()> {
        let row = sqlx::query(
            "SELECT t.name AS team_name, t.definition, r.goal, r.engine, r.task_id,
                    p.path AS project_path, p.default_branch, p.workspace_id
             FROM runs r JOIN teams t ON t.id = r.team_id
             JOIN projects p ON p.id = r.project_id
             WHERE r.id = $1",
        )
        .bind(run_id)
        .fetch_one(&self.db.pool)
        .await?;

        self.set_status(run_id, RunStatus::Starting).await?;

        let definition: serde_json::Value = row.get("definition");
        let workspace_id: Uuid = row.get("workspace_id");
        let goal: String = row.get::<Option<String>, _>("goal").unwrap_or_default();
        let team_name: String = row.get("team_name");
        let engine_id: String = row.get("engine");
        let engine = self
            .engine(&engine_id)
            .ok_or_else(|| anyhow::anyhow!("unknown engine {engine_id}"))?;

        let manager = self
            .resolve_member(workspace_id, definition.get("manager"))
            .await?
            .ok_or_else(|| anyhow::anyhow!("this organization has no manager"))?;
        let workers = self.resolve_members(workspace_id, &definition).await?;
        if workers.is_empty() {
            anyhow::bail!("this organization has no specialists to delegate to");
        }

        let task_id: Option<Uuid> = row.get("task_id");
        let worktree = self
            .worktree_for_run(
                run_id,
                task_id,
                &PathBuf::from(row.get::<String, _>("project_path")),
                &row.get::<String, _>("default_branch"),
                &slugify(&team_name),
            )
            .await?;
        let seq = SeqAlloc::starting_at(next_seq(&self.db, run_id).await?);

        self.post(run_id, None, "system", None, "status", &format!("{team_name} is on it."))
            .await?;

        // ── 1. The manager plans ────────────────────────────────────────
        let roster = workers
            .iter()
            .map(|m| format!("- {} ({}) — {}", m.name, m.title, m.description))
            .collect::<Vec<_>>()
            .join("\n");
        let plan_step = self
            .create_assignment(run_id, "plan", Some(&manager.name), "Plan the work", "")
            .await?;

        let plan_prompt = format!(
            "You manage a team working in this repository. Your goal:\n\n{goal}\n\n\
             Your specialists:\n{roster}\n\n\
             Inspect the repository first (Read/Grep/Glob), then split the goal into \
             assignments — as few as will do the job, at most one per specialist unless \
             someone clearly needs two. Assign each to the person best suited to it.\n\n\
             Reply with ONLY a JSON object, no prose and no markdown fences:\n\
             {{\"summary\": \"one sentence on your read of the goal and approach\", \
             \"tasks\": [{{\"key\": \"short_snake_case_id\", \"title\": \"short title\", \
             \"brief\": \"everything the specialist needs: what to change, where, and what \
             done looks like\", \"assignee\": \"exact specialist name from the roster\", \
             \"depends_on\": [\"keys of assignments that must finish first\"]}}]}}"
        );

        let plan_outcome = self
            .run_member(
                run_id,
                plan_step,
                &engine,
                &manager,
                &worktree.path,
                plan_prompt,
                None,
                MANAGER_TOOLS,
                PermissionMode::Reviewed,
                &seq,
            )
            .await?;

        if plan_outcome.status != RunStatus::Completed {
            let reason = plan_outcome.reason.unwrap_or_default();
            self.post(run_id, Some(plan_step), "system", None, "status",
                      &format!("Planning failed: {reason}")).await?;
            self.finish(run_id, plan_outcome.status, Some(reason)).await?;
            return Ok(());
        }

        let plan: Plan = match extract_json(&plan_outcome.output)
            .and_then(|v| serde_json::from_value(v).map_err(Into::into))
        {
            Ok(plan) => plan,
            Err(e) => {
                let note = format!("The manager's plan wasn't usable: {e}");
                self.post(run_id, Some(plan_step), "system", None, "status", &note).await?;
                self.finish(run_id, RunStatus::Failed, Some(note)).await?;
                return Ok(());
            }
        };

        if !plan.summary.is_empty() {
            self.post(run_id, Some(plan_step), &manager.name, None, "message", &plan.summary)
                .await?;
        }

        // ── 2. Materialize assignments ──────────────────────────────────
        let known: HashSet<&str> = workers.iter().map(|m| m.name.as_str()).collect();
        let mut assignments: Vec<(Uuid, PlannedTask)> = vec![];
        for task in plan.tasks {
            // A hallucinated name would silently misroute work; fall back to
            // the first specialist and say so.
            let assignee = if known.contains(task.assignee.as_str()) {
                task.assignee.clone()
            } else {
                let fallback = workers[0].name.clone();
                self.post(run_id, None, "system", None, "status", &format!(
                    "No specialist named \"{}\" — reassigning \"{}\" to {fallback}.",
                    task.assignee, task.title
                )).await?;
                fallback
            };
            let step_id = self
                .create_assignment(run_id, &task.key, Some(&assignee), &task.title, &task.brief)
                .await?;
            sqlx::query("UPDATE steps SET depends_on = $1 WHERE id = $2")
                .bind(&task.depends_on)
                .bind(step_id)
                .execute(&self.db.pool)
                .await?;
            self.post(run_id, Some(step_id), &manager.name, Some(&assignee), "assignment",
                      &format!("**{}** — {}", task.title, task.brief)).await?;
            assignments.push((step_id, PlannedTask { assignee, ..task }));
        }

        // ── 3. Specialists work, in dependency order ────────────────────
        let ordered = order_by_dependencies(&assignments);
        let mut results: Vec<String> = vec![];
        let mut failed: Option<String> = None;

        for index in ordered {
            let (step_id, task) = &assignments[index];
            let member = workers
                .iter()
                .find(|m| m.name == task.assignee)
                .unwrap_or(&workers[0]);

            let context = if results.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\nWhat your teammates have finished so far:\n{}",
                    results.join("\n\n")
                )
            };
            let prompt = format!(
                "You are {} ({}) on {team_name}. The team's goal:\n\n{goal}\n\n\
                 Your assignment — {}:\n{}{context}\n\n\
                 Work in this repository. Use mcp__aichip__post_message to tell the team \
                 what you're doing or what you found, and mcp__aichip__ask_manager if you \
                 hit a decision that isn't yours to make. Finish with a short summary of \
                 what you changed.",
                member.name, member.title, task.title, task.brief
            );

            self.post(run_id, Some(*step_id), &member.name, None, "status",
                      &format!("Starting: {}", task.title)).await?;

            let mut tools: Vec<String> = WORKER_TOOLS.iter().map(|s| s.to_string()).collect();
            tools.extend(ORG_TOOLS.iter().map(|s| s.to_string()));

            let outcome = self
                .run_member(
                    run_id,
                    *step_id,
                    &engine,
                    member,
                    &worktree.path,
                    prompt,
                    None,
                    &tools.iter().map(String::as_str).collect::<Vec<_>>(),
                    PermissionMode::AutoEdit,
                    &seq,
                )
                .await?;

            if outcome.status == RunStatus::Completed {
                self.post(run_id, Some(*step_id), &member.name, None, "result", &outcome.output)
                    .await?;
                results.push(format!("### {} (by {})\n{}", task.title, member.name, outcome.output));
            } else {
                let reason = outcome.reason.clone().unwrap_or_default();
                self.post(run_id, Some(*step_id), "system", None, "status",
                          &format!("{} could not finish \"{}\": {reason}", member.name, task.title))
                    .await?;
                failed = Some(format!("assignment \"{}\" {}", task.title, outcome.status.as_str()));
                break;
            }
        }

        // ── 4. The manager reviews ──────────────────────────────────────
        if failed.is_none() {
            let review_step = self
                .create_assignment(run_id, "review", Some(&manager.name), "Review the work", "")
                .await?;
            let review_prompt = format!(
                "Your team has finished work toward this goal:\n\n{goal}\n\n\
                 Their reports:\n\n{}\n\n\
                 Inspect what actually changed in the repository and give the user a short \
                 verdict: what was accomplished, anything you'd flag, and whether it's ready \
                 to review. Be direct — say so if something looks wrong or incomplete.",
                results.join("\n\n")
            );
            let review = self
                .run_member(
                    run_id,
                    review_step,
                    &engine,
                    &manager,
                    &worktree.path,
                    review_prompt,
                    plan_outcome.session_id.clone(),
                    MANAGER_TOOLS,
                    PermissionMode::Reviewed,
                    &seq,
                )
                .await?;
            if review.status == RunStatus::Completed {
                self.post(run_id, Some(review_step), &manager.name, None, "result", &review.output)
                    .await?;
            }
        }

        let status = match failed {
            None => {
                self.finish(run_id, RunStatus::Completed, None).await?;
                RunStatus::Completed
            }
            Some(reason) => {
                self.finish(run_id, RunStatus::Failed, Some(reason)).await?;
                RunStatus::Failed
            }
        };
        self.settle_task_for_run(task_id, status).await?;
        Ok(())
    }

    /// Run one member's step. Each gets its own MCP endpoint so the server
    /// knows which teammate is calling a tool.
    #[allow(clippy::too_many_arguments)]
    async fn run_member(
        self: &Arc<Self>,
        run_id: Uuid,
        step_id: Uuid,
        engine: &Arc<dyn aichip_engines::Engine>,
        member: &Member,
        cwd: &std::path::Path,
        prompt: String,
        resume: Option<String>,
        tools: &[&str],
        permission_mode: PermissionMode,
        seq: &SeqAlloc,
    ) -> anyhow::Result<super::orchestrator::StreamOutcome> {
        let agent = self.load_agent_by_name(&member.name).await?;
        let tier = agent.as_ref().map(|a| a.tier).unwrap_or(ModelTier::Medium);
        let mcp_config_path = match (&self.mcp_base_url, engine.id()) {
            (Some(base), "claude-code") => Some(
                write_mcp_config(base, &format!("mcp/org/{run_id}/{step_id}"), step_id).await?,
            ),
            _ => None,
        };

        let spec = RunSpec {
            cwd: cwd.to_path_buf(),
            prompt,
            model_tier: tier,
            model_id: self.tiers.model_for(tier).to_string(),
            resume_session_id: resume,
            permission_mode,
            allowed_tools: tools.iter().map(|s| s.to_string()).collect(),
            append_system_prompt: agent
                .as_ref()
                .and_then(|a| (!a.system_prompt.is_empty()).then(|| a.system_prompt.clone())),
            mcp_config_path,
            permission_prompt_tool: false,
            extra_env: HashMap::from([
                ("AICHIP_RUN_ID".to_string(), run_id.to_string()),
                ("MCP_TOOL_TIMEOUT".to_string(), "900000".to_string()),
            ]),
        };

        // The step row is what the live view reads to show who's busy, so
        // it has to bracket the actual work.
        sqlx::query("UPDATE steps SET status='running', started_at=now() WHERE id=$1")
            .bind(step_id)
            .execute(&self.db.pool)
            .await?;

        let outcome = self
            .stream_run(run_id, Some(step_id), seq, engine.clone(), spec, false)
            .await?;

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

        Ok(outcome)
    }

    /// A worker escalated a question. Wake the manager on its planning
    /// session so it answers with the full context of the plan it wrote.
    pub async fn consult_manager(
        self: &Arc<Self>,
        run_id: Uuid,
        step_id: Uuid,
        from_agent: &str,
        question: &str,
    ) -> anyhow::Result<String> {
        let asked: i64 = sqlx::query(
            "SELECT COUNT(*) AS n FROM org_messages WHERE step_id = $1 AND kind = 'question'",
        )
        .bind(step_id)
        .fetch_one(&self.db.pool)
        .await?
        .get("n");
        self.post(run_id, Some(step_id), from_agent, None, "question", question)
            .await?;
        if asked >= MAX_CONSULTS_PER_STEP {
            let answer = "You've already escalated several times — use your best judgement \
                          on this one and note the assumption in your summary.";
            self.post(run_id, Some(step_id), "system", Some(from_agent), "answer", answer)
                .await?;
            return Ok(answer.to_string());
        }

        let row = sqlx::query(
            "SELECT r.engine, p.path AS project_path,
                    (SELECT session_id FROM steps WHERE run_id = r.id AND step_key = 'plan') AS manager_session,
                    (SELECT assignee FROM steps WHERE run_id = r.id AND step_key = 'plan') AS manager_name,
                    r.goal
             FROM runs r JOIN projects p ON p.id = r.project_id WHERE r.id = $1",
        )
        .bind(run_id)
        .fetch_one(&self.db.pool)
        .await?;

        let engine_id: String = row.get("engine");
        let engine = self
            .engine(&engine_id)
            .ok_or_else(|| anyhow::anyhow!("unknown engine {engine_id}"))?;
        let manager_name: String = row
            .get::<Option<String>, _>("manager_name")
            .unwrap_or_else(|| "Manager".into());

        let spec = RunSpec {
            cwd: PathBuf::from(row.get::<String, _>("project_path")),
            prompt: format!(
                "{from_agent} is mid-assignment and asks:\n\n{question}\n\n\
                 Answer in two or three sentences so they can keep moving. Decide — don't \
                 hedge or hand the decision back."
            ),
            model_tier: ModelTier::Medium,
            model_id: self.tiers.model_for(ModelTier::Medium).to_string(),
            resume_session_id: row.get("manager_session"),
            permission_mode: PermissionMode::Reviewed,
            allowed_tools: vec![],
            append_system_prompt: None,
            mcp_config_path: None,
            permission_prompt_tool: false,
            extra_env: HashMap::new(),
        };

        // Deliberately not queued: a worker's tool call is blocked waiting
        // on this, and the run already holds its concurrency permit.
        let mut proc = engine.start(spec)?;
        let mut text = String::new();
        while let Some(event) = proc.events.recv().await {
            match event {
                aichip_shared::AichipEvent::RunCompleted { result_text, .. } => {
                    text = result_text;
                    break;
                }
                aichip_shared::AichipEvent::AssistantText { text: t } => {
                    if text.is_empty() {
                        text = t;
                    }
                }
                aichip_shared::AichipEvent::RunFailed { .. } => break,
                _ => {}
            }
        }
        if text.trim().is_empty() {
            text = "I couldn't weigh in right now — use your best judgement and flag the \
                    assumption in your summary."
                .to_string();
        }
        self.post(run_id, Some(step_id), &manager_name, Some(from_agent), "answer", &text)
            .await?;
        Ok(text)
    }

    /// Append to the team's conversation.
    pub async fn post(
        &self,
        run_id: Uuid,
        step_id: Option<Uuid>,
        from_agent: &str,
        to_agent: Option<&str>,
        kind: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO org_messages (run_id, step_id, from_agent, to_agent, kind, content)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(run_id)
        .bind(step_id)
        .bind(from_agent)
        .bind(to_agent)
        .bind(kind)
        .bind(content)
        .execute(&self.db.pool)
        .await?;
        Ok(())
    }

    async fn create_assignment(
        &self,
        run_id: Uuid,
        key: &str,
        assignee: Option<&str>,
        title: &str,
        brief: &str,
    ) -> anyhow::Result<Uuid> {
        let row = sqlx::query(
            "INSERT INTO steps (run_id, step_key, status, assignee, title, brief, started_at)
             VALUES ($1,$2,'queued',$3,$4,$5, now()) RETURNING id",
        )
        .bind(run_id)
        .bind(key)
        .bind(assignee)
        .bind(title)
        .bind(brief)
        .fetch_one(&self.db.pool)
        .await?;
        Ok(row.get("id"))
    }

    async fn load_agent_by_name(
        &self,
        name: &str,
    ) -> anyhow::Result<Option<super::orchestrator::BoundAgent>> {
        let row = sqlx::query(
            "SELECT workspace_id FROM agents WHERE name = $1 LIMIT 1",
        )
        .bind(name)
        .fetch_optional(&self.db.pool)
        .await?;
        match row {
            Some(r) => self.load_agent(r.get("workspace_id"), Some(name)).await,
            None => Ok(None),
        }
    }

    async fn resolve_member(
        &self,
        workspace_id: Uuid,
        id: Option<&serde_json::Value>,
    ) -> anyhow::Result<Option<Member>> {
        let Some(agent_id) = id.and_then(|v| v.as_str()).and_then(|s| Uuid::parse_str(s).ok())
        else {
            return Ok(None);
        };
        let row = sqlx::query(
            "SELECT name, description FROM agents WHERE id = $1 AND workspace_id = $2",
        )
        .bind(agent_id)
        .bind(workspace_id)
        .fetch_optional(&self.db.pool)
        .await?;
        Ok(row.map(|r| Member {
            name: r.get("name"),
            title: "Manager".to_string(),
            description: r.get("description"),
        }))
    }

    async fn resolve_members(
        &self,
        workspace_id: Uuid,
        definition: &serde_json::Value,
    ) -> anyhow::Result<Vec<Member>> {
        let entries = definition
            .get("members")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut members = vec![];
        for entry in entries {
            let Some(agent_id) = entry
                .get("agent_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
            else {
                continue;
            };
            let row = sqlx::query(
                "SELECT name, description FROM agents WHERE id = $1 AND workspace_id = $2",
            )
            .bind(agent_id)
            .bind(workspace_id)
            .fetch_optional(&self.db.pool)
            .await?;
            if let Some(r) = row {
                members.push(Member {
                    name: r.get("name"),
                    title: entry
                        .get("role")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Specialist")
                        .to_string(),
                    description: r.get("description"),
                });
            }
        }
        Ok(members)
    }
}

/// Kahn's algorithm over the manager's `depends_on` keys. Unknown keys are
/// ignored and anything left in a cycle is appended in authoring order —
/// a confused plan should still run, not hang.
fn order_by_dependencies(assignments: &[(Uuid, PlannedTask)]) -> Vec<usize> {
    let index_of: HashMap<&str, usize> = assignments
        .iter()
        .enumerate()
        .map(|(i, (_, t))| (t.key.as_str(), i))
        .collect();
    let mut done: HashSet<usize> = HashSet::new();
    let mut order = vec![];

    while order.len() < assignments.len() {
        let ready: Vec<usize> = (0..assignments.len())
            .filter(|i| !done.contains(i))
            .filter(|i| {
                assignments[*i]
                    .1
                    .depends_on
                    .iter()
                    .all(|d| index_of.get(d.as_str()).is_none_or(|j| done.contains(j)))
            })
            .collect();
        if ready.is_empty() {
            order.extend((0..assignments.len()).filter(|i| !done.contains(i)));
            break;
        }
        for i in ready {
            done.insert(i);
            order.push(i);
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(key: &str, depends_on: &[&str]) -> PlannedTask {
        PlannedTask {
            key: key.into(),
            title: key.into(),
            brief: String::new(),
            assignee: "A".into(),
            depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn ordered(tasks: Vec<PlannedTask>) -> Vec<String> {
        let assignments: Vec<(Uuid, PlannedTask)> =
            tasks.into_iter().map(|t| (Uuid::new_v4(), t)).collect();
        order_by_dependencies(&assignments)
            .into_iter()
            .map(|i| assignments[i].1.key.clone())
            .collect()
    }

    #[test]
    fn respects_declared_dependencies() {
        let order = ordered(vec![
            task("ui", &["api"]),
            task("api", &["schema"]),
            task("schema", &[]),
        ]);
        assert_eq!(order, ["schema", "api", "ui"]);
    }

    #[test]
    fn independent_work_keeps_authoring_order() {
        assert_eq!(ordered(vec![task("a", &[]), task("b", &[])]), ["a", "b"]);
    }

    #[test]
    fn a_hallucinated_dependency_does_not_strand_a_task() {
        assert_eq!(ordered(vec![task("a", &["nope"])]), ["a"]);
    }

    #[test]
    fn a_cyclic_plan_still_runs_everything() {
        let order = ordered(vec![task("a", &["b"]), task("b", &["a"]), task("c", &[])]);
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], "c");
    }
}
