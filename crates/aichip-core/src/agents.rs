//! Whether an agent takes work, and the one gate every start goes through.
//!
//! An agent used to have no state of its own: it existed or it did not, so the
//! only way to stop one was to delete it — which failed for any agent with a
//! run behind it, because `runs.agent_id` keeps its history. Now an agent is
//! active, paused, retired, or (reserved) pending approval, and the question
//! "may this agent start something?" has one answer, asked in one place.
//!
//! Every function that inserts a run asks it before the insert — a
//! source-scanning test below fails the build for one that does not, unless it
//! is on the list of runs that have no agent, with the reason why. Two checks
//! also run mid-flight, because a team run or a workflow outlives the click
//! that started it: a teammate's next assignment and a workflow's next step
//! each ask again, so pausing an agent stops its *next* piece of work wherever
//! that work was going to come from.

use sqlx::Row;
use uuid::Uuid;

use crate::db::Db;
use crate::runs::orchestrator::Orchestrator;

/// An agent that may not start anything right now, and why — in words for the
/// person who clicked Start.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{}", self.sentence())]
pub struct Unavailable {
    pub name: String,
    pub status: String,
    pub reason: Option<String>,
}

impl Unavailable {
    fn sentence(&self) -> String {
        let name = &self.name;
        match self.status.as_str() {
            "paused" => match self.reason.as_deref().filter(|r| !r.trim().is_empty()) {
                Some(reason) => {
                    format!("{name} is paused ({reason}) — resume the agent to start work")
                }
                None => format!("{name} is paused — resume the agent to start work"),
            },
            "retired" => format!("{name} is retired and takes no new work — assign someone else"),
            "pending_approval" => format!("{name} has not been approved yet"),
            other => format!("{name} is {other}"),
        }
    }
}

/// Refuse if any of these agents may not start work. Ids that name no agent
/// are not this gate's business — the caller's own lookup reports those.
pub async fn assert_can_run(db: &Db, agent_ids: &[Uuid]) -> anyhow::Result<()> {
    if agent_ids.is_empty() {
        return Ok(());
    }
    let row = sqlx::query(
        "SELECT name, status, pause_reason FROM agents
          WHERE id = ANY($1) AND status <> 'active'
          ORDER BY name LIMIT 1",
    )
    .bind(agent_ids)
    .fetch_optional(&db.pool)
    .await?;
    match row {
        None => Ok(()),
        Some(r) => Err(Unavailable {
            name: r.get("name"),
            status: r.get("status"),
            reason: r.get("pause_reason"),
        }
        .into()),
    }
}

/// The same question for a team: its manager and every member. All of them,
/// not "enough of them" — a pipeline cannot skip a stage, and an organization
/// whose roster silently shrank would plan around a specialist nobody said
/// was gone.
pub async fn assert_team_can_run(db: &Db, team_id: Uuid) -> anyhow::Result<()> {
    let definition: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT definition FROM teams WHERE id = $1")
            .bind(team_id)
            .fetch_optional(&db.pool)
            .await?;
    assert_can_run(db, &team_agent_ids(&definition.unwrap_or_default())).await
}

/// The same question for a workflow: every agent a step names, in the
/// workspace the workflow belongs to. Steps name agents by name, which is why
/// this cannot be [`assert_can_run`].
pub async fn assert_steps_can_run(
    db: &Db,
    workspace_id: Uuid,
    names: &[String],
) -> anyhow::Result<()> {
    if names.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM agents WHERE workspace_id = $1 AND name = ANY($2)")
            .bind(workspace_id)
            .bind(names)
            .fetch_all(&db.pool)
            .await?;
    assert_can_run(db, &ids).await
}

/// The gate once more at dispatch, for a run that was queued while its agent
/// could still run. A workflow is not asked here: each of its steps asks as
/// it starts (`load_agent`), which is the same question at a finer grain.
pub async fn assert_may_dispatch(db: &Db, run_id: Uuid) -> anyhow::Result<()> {
    let row = sqlx::query(
        "SELECT COALESCE(r.agent_id, t.agent_id) AS agent_id, r.team_id, r.workflow_id
           FROM runs r LEFT JOIN tasks t ON t.id = r.task_id
          WHERE r.id = $1",
    )
    .bind(run_id)
    .fetch_optional(&db.pool)
    .await?;
    // No such run is the caller's to report, with its own words.
    let Some(row) = row else { return Ok(()) };
    match (
        row.get::<Option<Uuid>, _>("team_id"),
        row.get::<Option<Uuid>, _>("workflow_id"),
    ) {
        (Some(team), _) => assert_team_can_run(db, team).await,
        (None, Some(_)) => Ok(()),
        (None, None) => {
            let agent: Option<Uuid> = row.get("agent_id");
            assert_can_run(db, agent.as_slice()).await
        }
    }
}

/// Every agent a team definition names: the manager, then the members.
/// Pure, so the definition's shape is tested without a database.
pub fn team_agent_ids(definition: &serde_json::Value) -> Vec<Uuid> {
    let manager = definition.get("manager").and_then(|v| v.as_str());
    let members = definition
        .get("members")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("agent_id").and_then(|v| v.as_str()));
    manager
        .into_iter()
        .chain(members)
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect()
}

/// Refuse an agent as a card's assignee. Looser than [`assert_can_run`] on
/// purpose: a paused agent can still be handed work to do when it is resumed
/// — that is half of what pausing is for — but a retired one never will.
pub async fn assert_assignable(db: &Db, agent_id: Uuid) -> anyhow::Result<()> {
    let row = sqlx::query("SELECT name, status, pause_reason FROM agents WHERE id = $1")
        .bind(agent_id)
        .fetch_optional(&db.pool)
        .await?;
    match row {
        Some(r)
            if matches!(
                r.get::<String, _>("status").as_str(),
                "retired" | "pending_approval"
            ) =>
        {
            Err(Unavailable {
                name: r.get("name"),
                status: r.get("status"),
                reason: r.get("pause_reason"),
            }
            .into())
        }
        _ => Ok(()),
    }
}

/// The runs an agent is doing right now, anywhere it can be doing them by
/// itself: its cards (bound to it, or a bake-off variant it was given) and its
/// comment replies. A team run or a workflow is not stopped — the agent is one
/// of several in it, and its next step there refuses instead.
async fn live_runs(db: &Db, agent_id: Uuid) -> anyhow::Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT r.id FROM runs r LEFT JOIN tasks t ON t.id = r.task_id
          WHERE COALESCE(r.agent_id, t.agent_id) = $1
            AND r.team_id IS NULL AND r.workflow_id IS NULL
            AND r.status NOT IN ('completed', 'failed', 'canceled')",
    )
    .bind(agent_id)
    .fetch_all(&db.pool)
    .await?)
}

impl Orchestrator {
    /// Stop an agent taking work, and — when asked — the work it is doing.
    /// Returns how many runs were stopped.
    pub async fn pause_agent(
        &self,
        agent_id: Uuid,
        reason: Option<&str>,
        stop_now: bool,
    ) -> anyhow::Result<usize> {
        let changed = sqlx::query(
            "UPDATE agents SET status = 'paused', pause_reason = $2, paused_at = now()
              WHERE id = $1 AND status IN ('active', 'paused')",
        )
        .bind(agent_id)
        .bind(reason.map(str::trim).filter(|r| !r.is_empty()))
        .execute(&self.db.pool)
        .await?
        .rows_affected();
        if changed == 0 {
            anyhow::bail!("only an active agent can be paused");
        }
        if stop_now {
            self.stop_agent_runs(agent_id).await
        } else {
            Ok(0)
        }
    }

    /// Let a paused agent take work again. What was refused while it was
    /// paused stays refused — a run that came up for dispatch meanwhile ended
    /// with the reason, and is started again by a person (Retry).
    pub async fn resume_agent(&self, agent_id: Uuid) -> anyhow::Result<()> {
        let changed = sqlx::query(
            "UPDATE agents SET status = 'active', pause_reason = NULL, paused_at = NULL
              WHERE id = $1 AND status = 'paused'",
        )
        .bind(agent_id)
        .execute(&self.db.pool)
        .await?
        .rows_affected();
        if changed == 0 {
            anyhow::bail!("only a paused agent can be resumed");
        }
        Ok(())
    }

    /// Retire an agent: no new work, gone from the pickers, its history kept
    /// — the runs, comments and spend it is named in still say who did them.
    /// Whatever it is doing stops. Returns how many runs were stopped.
    pub async fn retire_agent(&self, agent_id: Uuid) -> anyhow::Result<usize> {
        sqlx::query(
            "UPDATE agents SET status = 'retired', paused_at = COALESCE(paused_at, now())
              WHERE id = $1",
        )
        .bind(agent_id)
        .execute(&self.db.pool)
        .await?;
        self.stop_agent_runs(agent_id).await
    }

    async fn stop_agent_runs(&self, agent_id: Uuid) -> anyhow::Result<usize> {
        let runs = live_runs(&self.db, agent_id).await?;
        for run_id in &runs {
            // Same two halves as the Cancel button: interrupt what is
            // executing, close out what never started.
            if !self.cancel(*run_id) {
                self.cancel_idle(*run_id).await?;
            }
        }
        Ok(runs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_says_who_and_what_to_do() {
        let paused = Unavailable {
            name: "Ada".into(),
            status: "paused".into(),
            reason: Some("over budget".into()),
        };
        assert_eq!(
            paused.to_string(),
            "Ada is paused (over budget) — resume the agent to start work"
        );
        let retired = Unavailable {
            name: "Ada".into(),
            status: "retired".into(),
            reason: None,
        };
        assert!(retired.to_string().contains("assign someone else"));
    }

    #[test]
    fn a_team_names_its_manager_and_members() {
        let m = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let definition = serde_json::json!({
            "manager": m.to_string(),
            "members": [
                { "agent_id": a.to_string(), "role": "backend" },
                { "agent_id": "not-a-uuid" },
                { "role": "no agent" },
                { "agent_id": b.to_string() },
            ],
        });
        assert_eq!(team_agent_ids(&definition), vec![m, a, b]);
        assert!(team_agent_ids(&serde_json::Value::Null).is_empty());
    }

    /// The last `fn` declared in `source`: where it starts, and its name.
    fn enclosing_fn(source: &str) -> Option<(usize, &str)> {
        let mut found = None;
        let mut offset = 0;
        for line in source.split_inclusive('\n') {
            let mut rest = line.trim_start();
            for prefix in ["pub(crate) ", "pub(super) ", "pub ", "async "] {
                rest = rest.strip_prefix(prefix).unwrap_or(rest);
            }
            if let Some(after) = rest.strip_prefix("fn ") {
                let end = after
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(after.len());
                found = Some((offset, &after[..end]));
            }
            offset += line.len();
        }
        found
    }

    /// Runs with no agent behind them, and why. Anything else that inserts a
    /// run must ask the gate first.
    const NO_AGENT: &[(&str, &str)] = &[
        (
            "enqueue_chat_turn",
            "the assistant is not an agent; work it hands an agent starts through \
             enqueue_task, and a manager pass checks its agent in routines::dispatch",
        ),
        (
            "enqueue_kb_article",
            "generating an article runs as no agent",
        ),
        ("enqueue_research_run", "research runs as no agent"),
    ];

    /// Every function that inserts a run asks the gate before the insert, or
    /// says above why it has no agent to ask about.
    #[test]
    fn every_run_insert_asks_whether_its_agent_may_run() {
        let needle = concat!("INSERT INTO ", "runs");
        let roots = [
            concat!(env!("CARGO_MANIFEST_DIR"), "/src"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../aichip-server/src"),
        ];
        let mut files = vec![];
        for root in roots {
            let mut stack = vec![std::path::PathBuf::from(root)];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|e| e == "rs") {
                        files.push(path);
                    }
                }
            }
        }
        let mut seen = 0;
        let mut ungated = vec![];
        for path in files {
            let source = std::fs::read_to_string(&path).unwrap();
            for (at, _) in source.match_indices(needle) {
                seen += 1;
                let Some((start, name)) = enclosing_fn(&source[..at]) else {
                    ungated.push(format!("{}: outside any fn", path.display()));
                    continue;
                };
                let body = &source[start..at];
                // A test writing a run row as a fixture starts no agent.
                let before = source[..start].trim_end();
                let is_test = before.ends_with("#[test]") || before.ends_with("#[tokio::test]");
                if is_test
                    || body.contains("agents::assert_")
                    || NO_AGENT.iter().any(|(n, _)| *n == name)
                {
                    continue;
                }
                ungated.push(format!("{}: {name}", path.display()));
            }
        }
        assert!(
            seen >= 10,
            "the scan found only {seen} run inserts — is it looking in the right place?"
        );
        assert!(
            ungated.is_empty(),
            "these insert a run without asking agents::assert_can_run first:\n{}",
            ungated.join("\n")
        );
    }
}

/// Against a real database and the mock engine — see `crate::testdb`.
#[cfg(test)]
mod db_tests {
    use crate::testdb;
    use uuid::Uuid;

    async fn agent(t: &testdb::TestDb, ws: Uuid, name: &str) -> Uuid {
        sqlx::query_scalar("INSERT INTO agents (workspace_id, name) VALUES ($1, $2) RETURNING id")
            .bind(ws)
            .bind(name)
            .fetch_one(&t.db.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_paused_agent_starts_nothing_until_resumed() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        let (ws, project) = t.project(dir.path(), true).await;
        let ada = agent(&t, ws, "Ada").await;
        let card = t.card(project, "work").await;
        sqlx::query("UPDATE tasks SET agent_id = $2 WHERE id = $1")
            .bind(card)
            .bind(ada)
            .execute(&t.db.pool)
            .await
            .unwrap();

        orchestrator
            .pause_agent(ada, Some("over budget"), false)
            .await
            .unwrap();
        let refused = orchestrator.start_card(card).await.unwrap_err();
        let why = refused
            .downcast_ref::<super::Unavailable>()
            .expect("refused as unavailable");
        assert_eq!(why.reason.as_deref(), Some("over budget"));
        // Paused, not gone: it can still be handed work for later.
        super::assert_assignable(&t.db, ada).await.unwrap();

        orchestrator.resume_agent(ada).await.unwrap();
        orchestrator.start_card(card).await.unwrap();

        t.finish().await;
    }

    /// A run queued before the pause does not start after it: dispatch asks
    /// again, and the run ends with the reason instead of running.
    #[tokio::test]
    async fn a_run_queued_before_a_pause_ends_with_the_reason_instead_of_running() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        let (ws, project) = t.project(dir.path(), true).await;
        let ada = agent(&t, ws, "Ada").await;
        let card = t.card(project, "work").await;
        sqlx::query("UPDATE tasks SET agent_id = $2 WHERE id = $1")
            .bind(card)
            .bind(ada)
            .execute(&t.db.pool)
            .await
            .unwrap();

        orchestrator.set_queue_paused(true).await.unwrap();
        let run = orchestrator.start_card(card).await.unwrap();
        orchestrator.pause_agent(ada, None, false).await.unwrap();
        orchestrator.set_queue_paused(false).await.unwrap();

        t.until(
            "the queued run to end",
            "SELECT status IN ('completed','failed','canceled') FROM runs WHERE id = $1",
            run,
        )
        .await;
        let (status, reason): (String, Option<String>) =
            sqlx::query_as("SELECT status, error_reason FROM runs WHERE id = $1")
                .bind(run)
                .fetch_one(&t.db.pool)
                .await
                .unwrap();
        assert_eq!(status, "failed");
        assert!(reason.unwrap_or_default().contains("Ada is paused"));

        t.finish().await;
    }

    #[tokio::test]
    async fn a_retired_agent_takes_no_work_and_a_team_with_one_does_not_start() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        let (ws, project) = t.project(dir.path(), false).await;
        let lead = agent(&t, ws, "Lead").await;
        let dev = agent(&t, ws, "Dev").await;
        let team: Uuid = sqlx::query_scalar(
            "INSERT INTO teams (workspace_id, name, pattern, definition)
             VALUES ($1, 'crew', 'org', $2) RETURNING id",
        )
        .bind(ws)
        .bind(serde_json::json!({
            "manager": lead.to_string(),
            "members": [{ "agent_id": dev.to_string(), "role": "dev" }],
        }))
        .fetch_one(&t.db.pool)
        .await
        .unwrap();

        orchestrator.retire_agent(dev).await.unwrap();
        assert!(super::assert_assignable(&t.db, dev).await.is_err());
        let refused = orchestrator
            .enqueue_org_run(team, project, "ship it", false)
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("Dev is retired"), "{refused}");

        t.finish().await;
    }
}
