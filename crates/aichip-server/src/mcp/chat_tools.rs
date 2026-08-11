//! Workspace tools MCP endpoint for chat runs: the project assistant calls
//! these to create/start/inspect tasks. Every tool resolves through the
//! chat's own project, so a chat can never touch another project's data.

use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub async fn rpc(
    State(state): State<AppState>,
    Path(chat_id): Path<Uuid>,
    Json(req): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let Some(id) = req.get("id").cloned() else {
        return (StatusCode::ACCEPTED, Json(Value::Null));
    };
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");

    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "aichip", "version": env!("CARGO_PKG_VERSION") }
        }),
        "ping" => json!({}),
        "tools/list" => tools_list(),
        "tools/call" => {
            let name = req
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let args = req.pointer("/params/arguments").cloned().unwrap_or(json!({}));
            match call_tool(&state, chat_id, name, args).await {
                Ok(payload) => json!({
                    "content": [{ "type": "text", "text": payload.to_string() }]
                }),
                Err(msg) => json!({
                    "content": [{ "type": "text", "text": json!({"error": msg}).to_string() }],
                    "isError": true
                }),
            }
        }
        _ => {
            return (
                StatusCode::OK,
                Json(json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("method not found: {method}") }
                })),
            );
        }
    };
    (
        StatusCode::OK,
        Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
    )
}

pub fn tools_list() -> Value {
    let obj = |props: Value, required: Vec<&str>| {
        json!({ "type": "object", "properties": props, "required": required })
    };
    json!({
        "tools": [
            {
                "name": "create_task",
                "description": "Create a coding task on the project board. It runs in an isolated git worktree and its result appears for user review. Set start=true to launch immediately.",
                "inputSchema": obj(json!({
                    "title": { "type": "string" },
                    "prompt": { "type": "string", "description": "full instructions for the coding agent" },
                    "agent_name": { "type": "string", "description": "optional: bind a named agent from the library, spelled exactly as list_agents reports it. An unknown name is rejected. Omit it and a single agent the user @mentioned in their message is used instead." },
                    "model_tier": { "type": "string", "enum": ["easy", "medium", "complex"] },
                    "start": { "type": "boolean" }
                }), vec!["title", "prompt"])
            },
            { "name": "start_task", "description": "Start a backlog task.",
              "inputSchema": obj(json!({ "task_id": { "type": "string" } }), vec!["task_id"]) },
            { "name": "list_tasks", "description": "List this project's tasks with status.",
              "inputSchema": obj(json!({}), vec![]) },
            { "name": "get_task_status", "description": "Status, cost, and latest run of one task.",
              "inputSchema": obj(json!({ "task_id": { "type": "string" } }), vec!["task_id"]) },
            { "name": "list_agents", "description": "List available agents in this workspace.",
              "inputSchema": obj(json!({}), vec![]) },
            {
                "name": "cancel_task",
                "description": "Stop a task that is queued or running. Use it when the user says stop, or when you started something you should not have. It does not undo what the agent already wrote — the worktree and its diff survive for review. A task that already finished is reported as finished, which is not an error.",
                "inputSchema": obj(json!({ "task_id": { "type": "string" } }), vec!["task_id"])
            },
            {
                "name": "get_diff",
                "description": "What a task changed: one line per file, with lines added and removed. Use it to tell the user what a finished card did, or to judge whether it is worth their time to look at. Pass a path from that summary to read one file's actual diff, capped — it will be cut off, and asking for file after file to rebuild the whole change is not what this is for. A task that has not run has no diff.",
                "inputSchema": obj(json!({
                    "task_id": { "type": "string" },
                    "path": { "type": "string", "description": "optional: one file, spelled exactly as the summary reports it. Omit it for the summary." }
                }), vec!["task_id"])
            },
            {
                "name": "get_spend",
                "description": "What this project's runs have cost, and whether the queue will accept more work. Check it before starting anything large, and when the user asks what something cost. Runs still in flight report what they have spent so far, not what they will. Some engines never report a price; those runs are counted separately rather than folded in as zero. The budget and queue state are machine-wide, the totals are this project's.",
                "inputSchema": obj(json!({
                    "days": { "type": "integer", "description": "how far back to look, 1-365. Defaults to 7." }
                }), vec![])
            },
            {
                "name": "list_skills",
                "description": "The skills in this workspace: saved instructions for how the user wants a kind of job done. Use it to answer what skills they have, and to pass skill_name to create_task. You are given each skill's name and what it is for, never its text — the instructions are handed to whoever runs the task, so repeating them into a prompt would send them twice.",
                "inputSchema": obj(json!({}), vec![])
            },
            {
                "name": "move_task",
                "description": "File a card in a different column. Bookkeeping only: it changes nothing in git, and moving a card to done does not merge its branch. Use start_task to start a card. It refuses while the card is being worked on.",
                "inputSchema": obj(json!({
                    "task_id": { "type": "string" },
                    "column": { "type": "string", "enum": ["backlog", "review", "done"] }
                }), vec!["task_id", "column"])
            },
            // Deliberately absent: merge_task. `squash_merge` runs four git
            // commands in the user's real checkout, which is the same reason
            // Edit/Write/Bash are denied by name for chat runs — a front door
            // locked and a back door left open. There is also nowhere to ask:
            // chat runs carry `permission_prompt_tool: false`, so a
            // confirmation step has no channel to confirm on. Say the card is
            // ready and let them press Merge, where the diff is.
            //
            // And retry_task, whose only difference from start_task is
            // `fresh: true` — which deletes the worktree *and* the branch,
            // destroying the only copy of an unmerged diff.
        ]
    })
}

async fn chat_project(state: &AppState, chat_id: Uuid) -> Result<(Uuid, Uuid), String> {
    let row = sqlx::query(
        "SELECT p.id AS project_id, p.workspace_id FROM chats c
         JOIN projects p ON p.id = c.project_id WHERE c.id = $1",
    )
    .bind(chat_id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok((row.get("project_id"), row.get("workspace_id")))
}

async fn call_tool(
    state: &AppState,
    chat_id: Uuid,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    let (project_id, workspace_id) = chat_project(state, chat_id).await?;
    match name {
        "create_task" => {
            let title = unescape_html(
                args.get("title")
                    .and_then(Value::as_str)
                    .ok_or("title is required")?,
            );
            let title = title.as_str();
            let prompt = args
                .get("prompt")
                .and_then(Value::as_str)
                .ok_or("prompt is required")?;
            let tier = match args.get("model_tier").and_then(Value::as_str) {
                Some(t @ ("easy" | "medium" | "complex")) => t,
                _ => "medium",
            };
            let (agent_id, agent_name) =
                resolve_agent(state, chat_id, workspace_id, args.get("agent_name")).await?;
            // A skill the user named with `@`, on the same reasoning as the
            // agent: they asked for the work to be done a particular way, and
            // whether that survives should not depend on the assistant
            // remembering to say so.
            let skill = aichip_core::runs::mentions::latest_skills_for_chat(&state.db, chat_id)
                .await
                .map_err(|e| e.to_string())?;
            let skill_id = (skill.len() == 1).then(|| skill[0].0);
            let start = args.get("start").and_then(Value::as_bool).unwrap_or(false);

            let row = sqlx::query(
                "INSERT INTO tasks (project_id, title, prompt, model_tier, agent_id, skill_id, chat_id, board_column)
                 VALUES ($1,$2,$3,$4,$5,$8,$6, CASE WHEN $7 THEN 'running' ELSE 'backlog' END)
                 RETURNING id",
            )
            .bind(project_id)
            .bind(title)
            .bind(prompt)
            .bind(tier)
            .bind(agent_id)
            .bind(chat_id)
            .bind(start)
            .bind(skill_id)
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            let task_id: Uuid = row.get("id");

            let run_id = if start {
                Some(
                    state
                        .orchestrator
                        .enqueue_task(task_id)
                        .await
                        .map_err(|e| e.to_string())?,
                )
            } else {
                None
            };
            // The bound agent is echoed back so the assistant reports what
            // actually happened rather than what it asked for — those differ
            // exactly when it left `agent_name` off and the user's `@mention`
            // supplied it.
            Ok(json!({
                "task_id": task_id,
                "run_id": run_id,
                "started": start,
                "agent": agent_name,
                "skill": skill_id.and_then(|_| skill.first().map(|(_, n)| n.clone())),
            }))
        }
        "start_task" => {
            let task_id = parse_task_id(&args)?;
            ensure_task_in_project(state, task_id, project_id).await?;
            // The same vetting the Start button does. Without it the assistant
            // can queue a Reviewed card onto an engine that cannot ask for
            // permission, and get a run that refuses every tool call — the
            // capability gate is meant to refuse at the click, and this was a
            // way round it.
            crate::routes::tasks::vet_task(state, task_id)
                .await
                .map_err(|(_, message)| message)?;
            let run_id = state
                .orchestrator
                .enqueue_task(task_id)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query("UPDATE tasks SET board_column='running' WHERE id=$1")
                .bind(task_id)
                .execute(&state.db.pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({ "run_id": run_id, "started": true }))
        }
        "list_tasks" => {
            let rows = sqlx::query(
                "SELECT t.id, t.title, t.board_column, r.status AS run_status
                 FROM tasks t
                 LEFT JOIN LATERAL (SELECT status FROM runs WHERE task_id=t.id
                                    ORDER BY created_at DESC LIMIT 1) r ON TRUE
                 WHERE t.project_id=$1 ORDER BY t.created_at DESC",
            )
            .bind(project_id)
            .fetch_all(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({
                "tasks": rows.iter().map(|r| json!({
                    "task_id": r.get::<Uuid, _>("id"),
                    "title": r.get::<String, _>("title"),
                    "column": r.get::<String, _>("board_column"),
                    "run_status": r.get::<Option<String>, _>("run_status"),
                })).collect::<Vec<_>>()
            }))
        }
        "get_task_status" => {
            let task_id = parse_task_id(&args)?;
            ensure_task_in_project(state, task_id, project_id).await?;
            let row = sqlx::query(
                "SELECT t.title, t.board_column, r.status, r.cost_usd, r.error_reason
                 FROM tasks t
                 LEFT JOIN LATERAL (SELECT * FROM runs WHERE task_id=t.id
                                    ORDER BY created_at DESC LIMIT 1) r ON TRUE
                 WHERE t.id=$1",
            )
            .bind(task_id)
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({
                "title": row.get::<String, _>("title"),
                "column": row.get::<String, _>("board_column"),
                "run_status": row.get::<Option<String>, _>("status"),
                "cost_usd": row.get::<Option<f64>, _>("cost_usd"),
                "error": row.get::<Option<String>, _>("error_reason"),
            }))
        }
        "list_agents" => {
            let rows = sqlx::query(
                "SELECT name, description, model_tier FROM agents WHERE workspace_id=$1
                 ORDER BY name ASC",
            )
            .bind(workspace_id)
            .fetch_all(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({
                "agents": rows.iter().map(|r| json!({
                    "name": r.get::<String, _>("name"),
                    "description": r.get::<String, _>("description"),
                    "model_tier": r.get::<String, _>("model_tier"),
                })).collect::<Vec<_>>()
            }))
        }
        "cancel_task" => {
            let task_id = parse_task_id(&args)?;
            ensure_task_in_project(state, task_id, project_id).await?;
            // The task's run, never a run id from the model: `task_id` is the
            // only handle `ensure_task_in_project` can check, so a run id
            // would be an unguardable back door into another project. It also
            // means the assistant cannot reach its own chat run, which carries
            // a chat_id and a null task_id.
            let run_id: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM runs WHERE task_id=$1 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(task_id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;
            let Some(run_id) = run_id else {
                // A different fact from "already finished", and the assistant
                // should not report one as the other.
                return Err("this task has never been started, so there is nothing to cancel".into());
            };
            // The route handler itself, not a copy of it: its guards, its
            // wording, and anything added to it later apply here without a
            // second implementation drifting out of agreement.
            let axum::Json(v) = crate::routes::tasks::cancel_run(
                axum::extract::State(state.clone()),
                axum::extract::Path(run_id),
            )
            .await
            .map_err(|(_, message)| message)?;
            Ok(v)
        }
        "get_diff" => {
            let task_id = parse_task_id(&args)?;
            ensure_task_in_project(state, task_id, project_id).await?;
            let row = sqlx::query(
                "SELECT p.vcs, COALESCE(t.worktree_path, (
                            SELECT r.worktree_path FROM runs r
                             WHERE r.task_id = t.id AND r.worktree_path IS NOT NULL
                             ORDER BY r.created_at DESC LIMIT 1)) AS worktree,
                        COALESCE(p.default_branch, 'main') AS base
                   FROM tasks t JOIN projects p ON p.id = t.project_id
                  WHERE t.id = $1",
            )
            .bind(task_id)
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| e.to_string())?;

            // Refused with a reason rather than answered with an empty diff,
            // which to a model reads as "nothing changed".
            if row.get::<String, _>("vcs") != "git" {
                return Err("this project has no version control, so its tasks edit the folder directly — there is no diff".into());
            }
            let Some(worktree) = row.get::<Option<String>, _>("worktree") else {
                return Err("this task has not run yet, so there is nothing to diff".into());
            };
            let worktree = std::path::PathBuf::from(worktree);
            let base: String = row.get("base");

            let stats = state
                .orchestrator
                .worktrees
                .diff_stat(&worktree, &base)
                .await
                .map_err(|e| e.to_string())?;

            match args.get("path").and_then(Value::as_str) {
                Some(path) => {
                    if !stats.iter().any(|f| f.path == path) {
                        let names: Vec<_> =
                            stats.iter().take(20).map(|f| f.path.clone()).collect();
                        return Err(format!(
                            "no file called {path} changed in this task. These did: {}",
                            names.join(", ")
                        ));
                    }
                    let full = state
                        .orchestrator
                        .worktrees
                        .diff_file(&worktree, &base, path)
                        .await
                        .map_err(|e| e.to_string())?;
                    let (text, dropped) = clip(&full, MAX_DIFF_CHARS);
                    Ok(json!({
                        "path": path,
                        "diff": text,
                        "truncated": dropped > 0,
                    }))
                }
                None => Ok(summarize(stats)),
            }
        }
        "get_spend" => {
            let days = args
                .get("days")
                .and_then(Value::as_i64)
                .unwrap_or(7)
                .clamp(1, 365) as i32;
            let totals = aichip_core::spend::for_project(&state.db, project_id, days)
                .await
                .map_err(|e| e.to_string())?;
            let gate = state.orchestrator.queue_gate().await;
            Ok(json!({
                "days": days,
                "project": totals,
                // Named apart from the project totals on purpose: the cap and
                // the queue are the whole machine's, not this project's.
                "queue": match &gate {
                    aichip_core::runs::orchestrator::QueueGate::Open => json!({ "state": "open" }),
                    aichip_core::runs::orchestrator::QueueGate::Paused => json!({ "state": "paused" }),
                    aichip_core::runs::orchestrator::QueueGate::OverBudget { spent_today, cap_usd } =>
                        json!({ "state": "over_budget", "spent_today_usd": spent_today, "cap_usd": cap_usd }),
                },
            }))
        }
        "list_skills" => {
            let skills = aichip_core::skills::list(&state.db, workspace_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(json!({
                // Names and descriptions only. The bodies are capped at 4000
                // characters each and are fenced and neutralised at run time
                // precisely because they are untrusted text — handing them to
                // the assistant outside that fence would undo the work, and it
                // has no use for them: `create_task` binds by id and the text
                // is folded in when the run starts.
                "skills": skills.iter().filter(|s| s.enabled).map(|s| json!({
                    "name": s.name,
                    "description": s.description,
                })).collect::<Vec<_>>()
            }))
        }
        "move_task" => {
            let task_id = parse_task_id(&args)?;
            ensure_task_in_project(state, task_id, project_id).await?;
            let column = args
                .get("column")
                .and_then(Value::as_str)
                .ok_or("column must be backlog, review or done")?;
            // "running" is deliberately not offered: it would relabel a card
            // that has no run, which is worse than the stranded state it looks
            // like it fixes. Starting work is `start_task`.
            if !["backlog", "review", "done"].contains(&column) {
                return Err(format!(
                    "{column} is not a column you can file a card in — use backlog, review or done, and start_task to start one"
                ));
            }
            let axum::Json(_) = crate::routes::tasks::move_task(
                axum::extract::State(state.clone()),
                axum::extract::Path(task_id),
                axum::Json(crate::routes::tasks::MoveTask::to_column(column)),
            )
            .await
            .map_err(|(_, message)| message)?;
            // Says "filed", never "landed": a user reading "done" in chat must
            // not come away thinking their code merged.
            Ok(json!({ "filed": true, "column": column }))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Files listed in one summary, and characters of one file's diff.
///
/// A five-thousand-line change must not be able to reach the model in one
/// call. The failure to avoid is not a truncated answer — it is an assistant
/// that calls `get_diff` twelve times to rebuild what it was capped out of,
/// which is why the tool description says not to.
const MAX_DIFF_FILES: usize = 200;
const MAX_DIFF_CHARS: usize = 12_000;

/// One line per file, biggest first, saying plainly what did not fit.
fn summarize(mut stats: Vec<aichip_core::worktrees::manager::FileStat>) -> Value {
    let files_changed = stats.len();
    let added: i64 = stats.iter().map(|f| f.added).sum();
    let removed: i64 = stats.iter().map(|f| f.removed).sum();
    stats.sort_by_key(|f| -(f.added + f.removed));
    let more = files_changed.saturating_sub(MAX_DIFF_FILES);
    stats.truncate(MAX_DIFF_FILES);
    json!({
        "files_changed": files_changed,
        "added": added,
        "removed": removed,
        "files": stats,
        // Named rather than silently dropped, on the same reasoning the
        // knowledge base uses: a list that stops without saying so reads as a
        // complete list.
        "more_files": more,
    })
}

/// Cut to a budget on a line boundary, saying how much was left out.
///
/// The same shape and marker as `brain`, `skills` and the knowledge base use.
fn clip(text: &str, budget: usize) -> (String, usize) {
    if text.chars().count() <= budget {
        return (text.to_string(), 0);
    }
    let kept: String = text.chars().take(budget).collect();
    let cut = kept.rfind('\n').map(|i| &kept[..i]).unwrap_or(&kept).to_string();
    let dropped = text.chars().count() - cut.chars().count();
    (
        format!("{cut}\n[truncated — {dropped} more characters]"),
        dropped,
    )
}

/// Which agent a new task binds to: what the assistant asked for, or failing
/// that, who the user named with `@` in the message being answered.
///
/// Two behaviours worth stating, because both used to be absent:
///
/// * **An unknown `agent_name` is an error**, not a shrug. It used to resolve
///   to `NULL`, so a single typo produced an unassigned task while the
///   assistant cheerfully reported it had assigned one. The error lists the
///   real names, which is something the model can act on.
/// * **A single `@mention` binds even when the model forgets to pass it
///   through.** That is the whole point of resolving the mention at send time:
///   the user's instruction does not depend on the model relaying it. Two or
///   more mentions are left to the model, because "which of these two tasks is
///   whose" is a question only the request can answer — and the prompt block
///   `mentions::augment_prompt` adds tells it to answer it.
async fn resolve_agent(
    state: &AppState,
    chat_id: Uuid,
    workspace_id: Uuid,
    asked: Option<&Value>,
) -> Result<(Option<Uuid>, Option<String>), String> {
    if let Some(name) = asked.and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
        // Matched without regard to case, because everything upstream is:
        // `@frontend` finds the agent called Frontend, and a model echoing the
        // user's own typing back must not turn a mention that already resolved
        // into a hard error. `agents_ws_name` is unique per workspace, and two
        // names differing only in case would be a library nobody could use.
        let row = sqlx::query(
            "SELECT id, name FROM agents WHERE workspace_id=$1 AND lower(name)=lower($2)",
        )
        .bind(workspace_id)
        .bind(name)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
        return match row {
            Some(r) => Ok((Some(r.get("id")), Some(r.get("name")))),
            None => {
                let known = sqlx::query("SELECT name FROM agents WHERE workspace_id=$1 ORDER BY name")
                    .bind(workspace_id)
                    .fetch_all(&state.db.pool)
                    .await
                    .map_err(|e| e.to_string())?
                    .iter()
                    .map(|r| format!("\"{}\"", r.get::<String, _>("name")))
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(if known.is_empty() {
                    format!("no agent named \"{name}\" — this workspace has no agents yet")
                } else {
                    format!("no agent named \"{name}\". The agents here are: {known}")
                })
            }
        };
    }

    let mentioned = aichip_core::runs::mentions::latest_for_chat(&state.db, chat_id)
        .await
        .map_err(|e| e.to_string())?;
    match mentioned.len() {
        1 => Ok((Some(mentioned[0].0), Some(mentioned[0].1.clone()))),
        _ => Ok((None, None)),
    }
}

/// Undo HTML escaping a model applied to a plain-text field.
///
/// Not hypothetical: `Snakes &amp; Ladders board game app` is in this
/// database, from an assistant that escaped its own title on the way into
/// `create_task`. Nothing downstream renders a card title as HTML — the board,
/// the drawer and the commit message all treat it as text — so the escape has
/// no reader and shows up verbatim in every one of them, including the git
/// history a merge writes.
///
/// The five XML entities only, and applied once. A title that genuinely
/// contains `&amp;` is a title about HTML, which is rarer than a title about
/// something and someone else.
fn unescape_html(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        // Last, or `&amp;lt;` would decode two steps into `<`.
        .replace("&amp;", "&")
}

fn parse_task_id(args: &Value) -> Result<Uuid, String> {
    args.get("task_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| "task_id must be a UUID".to_string())
}

async fn ensure_task_in_project(
    state: &AppState,
    task_id: Uuid,
    project_id: Uuid,
) -> Result<(), String> {
    let row = sqlx::query("SELECT project_id FROM tasks WHERE id=$1")
        .bind(task_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(|e| e.to_string())?;
    match row {
        Some(r) if r.get::<Uuid, _>("project_id") == project_id => Ok(()),
        Some(_) => Err("task belongs to a different project".into()),
        None => Err("no such task".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_exposes_every_tool_the_assistant_has() {
        let v = tools_list();
        let names: Vec<&str> = v["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                // What it can start.
                "create_task",
                "start_task",
                // What it can see.
                "list_tasks",
                "get_task_status",
                "list_agents",
                // And what it can do about the result, which is the half that
                // was missing: it could spend $143 and then had no tool to
                // stop it, read it, or say what it cost.
                "cancel_task",
                "get_diff",
                "get_spend",
                "list_skills",
                "move_task",
            ]
        );
    }

    /// The schema is the only place the model learns these two rules, and both
    /// are behaviour `resolve_agent` will actually enforce — a description that
    /// drifts from it is how a model ends up retrying a call that can never
    /// succeed, or omitting an argument thinking something else will fill it in.
    #[test]
    fn a_title_the_model_escaped_is_stored_as_the_words_it_meant() {
        // `Snakes &amp; Ladders board game app` is really in the database.
        assert_eq!(
            super::unescape_html("Snakes &amp; Ladders board game app"),
            "Snakes & Ladders board game app"
        );
        assert_eq!(super::unescape_html("Fix &lt;head&gt; ordering"), "Fix <head> ordering");
        // Applied once: `&amp;` is decoded last, so this stays as the text it
        // was rather than decoding twice into `<`.
        assert_eq!(super::unescape_html("&amp;lt;"), "&lt;");
        // Untouched when there is nothing to undo.
        assert_eq!(super::unescape_html("Add a README"), "Add a README");
    }

    #[test]
    fn create_task_tells_the_model_what_agent_name_does() {
        let v = tools_list();
        let described = v["tools"][0]["inputSchema"]["properties"]["agent_name"]["description"]
            .as_str()
            .unwrap();
        assert!(described.contains("unknown name is rejected"));
        assert!(described.contains("@mention"));
    }

    /// The failure this change can introduce, and it is silent.
    ///
    /// A tool advertised by `tools/list` but missing from `CHAT_ALLOWED_TOOLS`
    /// is one the model reaches for and is refused — and chat runs carry
    /// `permission_prompt_tool: false`, so there is no prompt to answer. The
    /// call simply fails and the assistant reports that the workspace is
    /// broken. The two lists live in different crates, which is exactly why
    /// nothing else would catch it.
    #[test]
    fn every_tool_the_assistant_is_offered_is_one_it_is_pre_approved_to_call() {
        let offered: std::collections::HashSet<String> = tools_list()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| format!("mcp__aichip__{}", t["name"].as_str().unwrap()))
            .collect();
        let allowed: std::collections::HashSet<String> =
            aichip_core::runs::orchestrator::CHAT_ALLOWED_TOOLS
                .iter()
                .filter(|t| t.starts_with("mcp__aichip__"))
                .map(|t| t.to_string())
                .collect();
        // Both directions: a grant for a tool that does not exist is usually a
        // rename done on one side only.
        assert_eq!(offered, allowed);
    }

    /// Keeps the omission a decision the codebase holds, rather than a gap the
    /// next contributor fills in.
    #[test]
    fn the_chat_assistant_is_offered_no_way_to_merge() {
        for t in tools_list()["tools"].as_array().unwrap() {
            let name = t["name"].as_str().unwrap();
            assert!(!name.contains("merge"), "{name} lands code from chat");
            assert!(!name.contains("retry"), "{name} can discard an unmerged diff");
        }
    }

    #[test]
    fn the_descriptions_say_the_parts_that_are_easy_to_get_wrong() {
        let tools = tools_list();
        let by_name = |n: &str| -> String {
            tools["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == n)
                .unwrap()["description"]
                .as_str()
                .unwrap()
                .to_string()
        };
        // A model that does not know the diff is capped will try to rebuild it.
        assert!(by_name("get_diff").contains("capped"));
        // And one that thinks "done" merges will tell the user their code landed.
        assert!(by_name("move_task").contains("does not merge"));
        // Cancelling must not read as undoing.
        assert!(by_name("cancel_task").contains("does not undo"));
    }

    #[test]
    fn a_summary_names_the_files_it_left_out() {
        let stats: Vec<_> = (0..MAX_DIFF_FILES + 5)
            .map(|i| aichip_core::worktrees::manager::FileStat {
                path: format!("f{i}.rs"),
                added: i as i64,
                removed: 0,
                binary: false,
            })
            .collect();
        let v = summarize(stats);
        assert_eq!(v["files_changed"], MAX_DIFF_FILES + 5);
        assert_eq!(v["files"].as_array().unwrap().len(), MAX_DIFF_FILES);
        assert_eq!(v["more_files"], 5);
        // Biggest first, so what is dropped is the least interesting.
        assert_eq!(v["files"][0]["path"], format!("f{}.rs", MAX_DIFF_FILES + 4));
    }

    #[test]
    fn a_short_diff_is_not_marked_truncated() {
        let (text, dropped) = clip("one\ntwo\n", 100);
        assert_eq!(text, "one\ntwo\n");
        assert_eq!(dropped, 0);
    }

    #[test]
    fn a_long_diff_says_how_much_it_left_out() {
        let long = "a line of diff\n".repeat(2000);
        let (text, dropped) = clip(&long, 200);
        assert!(dropped > 0);
        assert!(text.contains("[truncated —"), "{text}");
        // Cut on a line boundary, so the last line shown is a whole one.
        let body = text.lines().next().unwrap();
        assert_eq!(body, "a line of diff");
    }
}
