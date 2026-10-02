//! Heartbeats: agents that pull their own next card.
//!
//! An agent with a heartbeat (`agents.heartbeat_secs`, a person's standing
//! decision like `start_when_unblocked`) checks in on a timer — and early,
//! when something it cares about happens: a card is handed to it, or one of
//! its cards is unblocked. Each beat does at most one thing:
//!
//! 1. Nothing, if the agent is paused or retired, or already working.
//! 2. If the agent runs a project's manager routine, fire that — through
//!    `routines::fire`, subject to the routine's own cooldown and daily cap.
//! 3. Otherwise start its next assigned card that is in the backlog and not
//!    blocked — through `start_card`, the Start button's own vet and door, so
//!    budgets, agent limits, capabilities and the Full Auto opt-in all apply.
//! 4. With nothing to do, record an idle beat. No model is called; an idle
//!    heartbeat costs nothing.
//!
//! Every beat is written to `heartbeats`, so "is it picking up work or just
//! idling?" has an answer.

use crate::db::Db;
use crate::runs::orchestrator::Orchestrator;
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

/// Beats kept per agent.
pub const KEEP: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Timer,
    Wake,
}

impl Reason {
    fn as_str(self) -> &'static str {
        match self {
            Reason::Timer => "timer",
            Reason::Wake => "wake",
        }
    }
}

/// What one beat did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Started a card.
    Started { task_id: Uuid, run_id: Uuid },
    /// Fired the manager routine it runs.
    Fired,
    /// Nothing to do.
    Idle,
    /// Already working.
    Busy,
    /// Had work, but something said not now (budget, limits, a vet) — with
    /// the card it was about, when there was one.
    Held { why: String, task_id: Option<Uuid> },
    /// Paused, retired, or its heartbeat was turned off.
    Paused,
}

impl Outcome {
    fn as_str(&self) -> &'static str {
        match self {
            Outcome::Started { .. } => "started",
            Outcome::Fired => "fired",
            Outcome::Idle => "idle",
            Outcome::Busy => "busy",
            Outcome::Held { .. } => "held",
            Outcome::Paused => "paused",
        }
    }
}

/// One beat, recorded. Never fails the scheduler: an error is recorded as
/// held.
pub async fn beat(orch: &Orchestrator, agent: Uuid, reason: Reason) -> Outcome {
    let outcome = match try_beat(orch, agent).await {
        Ok(o) => o,
        Err(e) => Outcome::Held {
            why: e.to_string(),
            task_id: None,
        },
    };
    let (task, run, detail) = match &outcome {
        Outcome::Started { task_id, run_id } => (Some(*task_id), Some(*run_id), String::new()),
        Outcome::Held { why, task_id } => (*task_id, None, why.chars().take(300).collect()),
        _ => (None, None, String::new()),
    };
    let recorded = sqlx::query(
        "INSERT INTO heartbeats (agent_id, reason, outcome, task_id, run_id, detail)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(agent)
    .bind(reason.as_str())
    .bind(outcome.as_str())
    .bind(task)
    .bind(run)
    .bind(detail)
    .execute(&orch.db.pool)
    .await;
    if let Err(e) = recorded {
        tracing::warn!(%agent, error = %e, "could not record a heartbeat");
    }
    let _ = sqlx::query(
        "DELETE FROM heartbeats WHERE agent_id = $1 AND id NOT IN
           (SELECT id FROM heartbeats WHERE agent_id = $1 ORDER BY at DESC, id DESC LIMIT $2)",
    )
    .bind(agent)
    .bind(KEEP)
    .execute(&orch.db.pool)
    .await;
    outcome
}

async fn try_beat(orch: &Orchestrator, agent: Uuid) -> anyhow::Result<Outcome> {
    let Some(row) = sqlx::query(
        "SELECT name, workspace_id, status, max_concurrent, heartbeat_secs FROM agents WHERE id = $1",
    )
    .bind(agent)
    .fetch_optional(&orch.db.pool)
    .await?
    else {
        return Ok(Outcome::Paused);
    };
    // A wake raised before a person turned the heartbeat off is not a reason
    // to start work for them: every beat asks, whatever raised it.
    if row.get::<String, _>("status") != "active"
        || row.get::<Option<i32>, _>("heartbeat_secs").is_none()
    {
        return Ok(Outcome::Paused);
    }
    // One card at a time unless a person allowed more — counting a step it
    // is doing for a team, which names it rather than pointing at it.
    let live: i64 = sqlx::query_scalar(
        "SELECT
            (SELECT count(*) FROM runs r LEFT JOIN tasks t ON t.id = r.task_id
              WHERE COALESCE(r.agent_id, t.agent_id) = $1
                AND r.team_id IS NULL AND r.workflow_id IS NULL
                AND r.status IN ('queued', 'starting', 'running', 'waiting_permission', 'rate_limited'))
          + (SELECT count(*) FROM steps s JOIN runs r ON r.id = s.run_id
               LEFT JOIN projects p ON p.id = r.project_id
              WHERE s.assignee = $2
                AND s.status IN ('queued', 'starting', 'running', 'waiting_permission', 'rate_limited')
                AND r.status NOT IN ('completed', 'failed', 'canceled')
                AND (p.workspace_id IS NULL OR p.workspace_id = $3))",
    )
    .bind(agent)
    .bind(row.get::<String, _>("name"))
    .bind(row.get::<Option<Uuid>, _>("workspace_id"))
    .fetch_one(&orch.db.pool)
    .await?;
    let allowed = row
        .get::<Option<i32>, _>("max_concurrent")
        .unwrap_or(1)
        .max(1) as i64;
    if live >= allowed {
        return Ok(Outcome::Busy);
    }

    // A manager agent's job is its pass — each of them, in turn: the one
    // fired longest ago first, so an agent managing two projects does not
    // spend every beat on the same one.
    let routines: Vec<Uuid> = sqlx::query_scalar(
        "SELECT rt.id FROM routines rt
          WHERE rt.agent_id = $1 AND rt.kind = 'manage' AND rt.enabled
          ORDER BY (SELECT max(rr.fired_at) FROM routine_runs rr WHERE rr.routine_id = rt.id)
                   NULLS FIRST, rt.id",
    )
    .bind(agent)
    .fetch_all(&orch.db.pool)
    .await?;
    if !routines.is_empty() {
        for routine in routines {
            if orch.may_wake_manager(routine).await? {
                crate::routines::fire(&orch.db, orch, routine, "heartbeat").await?;
                return Ok(Outcome::Fired);
            }
        }
        return Ok(Outcome::Busy);
    }

    // Otherwise its next card: assigned to it, in the backlog, with nothing
    // it waits on still unlanded, and not a sub-task a team run still holds.
    let candidates: Vec<Uuid> = sqlx::query_scalar(
        "SELECT t.id FROM tasks t
          WHERE t.agent_id = $1 AND t.team_id IS NULL AND t.board_column = 'backlog'
            AND NOT EXISTS (SELECT 1 FROM task_deps d JOIN tasks b ON b.id = d.blocked_by
                             WHERE d.task_id = t.id AND b.board_column <> 'done')
            AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.task_id = t.id
                               AND r.status NOT IN ('completed', 'failed', 'canceled'))
            AND NOT EXISTS (SELECT 1 FROM steps s JOIN runs r ON r.id = s.run_id
                             WHERE s.task_id = t.id
                               AND s.status IN ('queued', 'starting', 'running',
                                                'waiting_permission', 'rate_limited')
                               AND r.status NOT IN ('completed', 'failed', 'canceled'))
          ORDER BY t.position, t.created_at LIMIT 20",
    )
    .bind(agent)
    .fetch_all(&orch.db.pool)
    .await?;
    // A card that can never start as it stands — Reviewed on an engine that
    // cannot ask — is passed over, not waited on: it would pin every card
    // behind it, beat after beat, and log it as a hold that would pass.
    let mut stuck: Option<(Uuid, String)> = None;
    for task_id in candidates {
        if let Some(why) = orch.vet_card(task_id).await? {
            stuck.get_or_insert((task_id, why));
            continue;
        }
        // Anything start_card refuses now — a budget, the agent's limits —
        // would refuse every card of this agent alike, so it is the beat's
        // answer.
        return Ok(match orch.start_card(task_id).await {
            Ok(run_id) => Outcome::Started { task_id, run_id },
            Err(e) => Outcome::Held {
                why: e.to_string(),
                task_id: Some(task_id),
            },
        });
    }
    Ok(match stuck {
        Some((task_id, why)) => Outcome::Held {
            why,
            task_id: Some(task_id),
        },
        None => Outcome::Idle,
    })
}

/// Raise a wake for an agent's heartbeat — only for an agent that has one.
/// Coalesced like every wake.
pub async fn wake(db: &Db, agent: Uuid, kind: &str, task_id: Option<Uuid>) {
    let r = sqlx::query(
        "INSERT INTO wakeups (agent_id, kind, task_id)
         SELECT id, $2, $3 FROM agents WHERE id = $1 AND heartbeat_secs IS NOT NULL AND status = 'active'
         ON CONFLICT (agent_id, kind, COALESCE(task_id, '00000000-0000-0000-0000-000000000000'::uuid))
            WHERE consumed_at IS NULL AND agent_id IS NOT NULL
         DO UPDATE SET count = wakeups.count + 1, last_at = now()",
    )
    .bind(agent)
    .bind(kind)
    .bind(task_id)
    .execute(&db.pool)
    .await;
    if let Err(e) = r {
        tracing::warn!(%agent, error = %e, "could not wake an agent");
    }
}

/// One beat as the agent sheet shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Beat {
    pub at: chrono::DateTime<chrono::Utc>,
    pub reason: String,
    pub outcome: String,
    pub task_id: Option<Uuid>,
    pub task_title: Option<String>,
    pub project_id: Option<Uuid>,
    pub detail: String,
}

pub async fn recent(db: &Db, agent: Uuid, limit: i64) -> anyhow::Result<Vec<Beat>> {
    let rows = sqlx::query(
        "SELECT h.at, h.reason, h.outcome, h.task_id, t.title, t.project_id, h.detail
           FROM heartbeats h LEFT JOIN tasks t ON t.id = h.task_id
          WHERE h.agent_id = $1 ORDER BY h.at DESC, h.id DESC LIMIT $2",
    )
    .bind(agent)
    .bind(limit)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Beat {
            at: r.get("at"),
            reason: r.get("reason"),
            outcome: r.get("outcome"),
            task_id: r.get("task_id"),
            task_title: r.get("title"),
            project_id: r.get("project_id"),
            detail: r.get("detail"),
        })
        .collect())
}

/// The workspace's recent beats that did something, newest first.
pub async fn recent_in(
    db: &Db,
    workspace: Uuid,
    limit: i64,
) -> anyhow::Result<Vec<(String, Beat)>> {
    let rows = sqlx::query(
        "SELECT a.name, h.at, h.reason, h.outcome, h.task_id, t.title, t.project_id, h.detail
           FROM heartbeats h JOIN agents a ON a.id = h.agent_id
           LEFT JOIN tasks t ON t.id = h.task_id
          WHERE a.workspace_id = $1 AND h.outcome IN ('started', 'fired', 'held')
          ORDER BY h.at DESC, h.id DESC LIMIT $2",
    )
    .bind(workspace)
    .bind(limit)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get("name"),
                Beat {
                    at: r.get("at"),
                    reason: r.get("reason"),
                    outcome: r.get("outcome"),
                    task_id: r.get("task_id"),
                    task_title: r.get("title"),
                    project_id: r.get("project_id"),
                    detail: r.get("detail"),
                },
            )
        })
        .collect())
}

impl Orchestrator {
    /// Every heartbeat that is due, and every agent woken early. Each
    /// scheduler tick.
    pub async fn heartbeats(&self) {
        // Due on the timer: claimed with a compare-and-set on the last beat,
        // so a slow tick and the next one cannot both beat the same agent.
        let due: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE agents SET last_heartbeat_at = now()
              WHERE heartbeat_secs IS NOT NULL AND status = 'active'
                AND (last_heartbeat_at IS NULL
                     OR last_heartbeat_at <= now() - make_interval(secs => heartbeat_secs))
              RETURNING id",
        )
        .fetch_all(&self.db.pool)
        .await
        .unwrap_or_default();
        for agent in &due {
            beat(self, *agent, Reason::Timer).await;
        }
        // Woken early: consumed first, so one wake is one beat.
        let woken: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE wakeups SET consumed_at = now()
              WHERE consumed_at IS NULL AND agent_id IS NOT NULL
              RETURNING agent_id",
        )
        .fetch_all(&self.db.pool)
        .await
        .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        for agent in woken {
            if due.contains(&agent) || !seen.insert(agent) {
                continue;
            }
            beat(self, agent, Reason::Wake).await;
        }
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    struct Fixture {
        t: testdb::TestDb,
        orch: std::sync::Arc<Orchestrator>,
        _dir: tempfile::TempDir,
        project: Uuid,
        agent: Uuid,
    }

    async fn fixture() -> Option<Fixture> {
        let t = testdb::fresh().await?;
        let dir = tempfile::tempdir().unwrap();
        let orch = t.orchestrator(dir.path());
        orch.set_queue_paused(true).await.unwrap();
        let (ws, project) = t.project(dir.path(), false).await;
        let agent: Uuid = sqlx::query_scalar(
            "INSERT INTO agents (workspace_id, name, engine, heartbeat_secs) VALUES ($1, 'Ada', 'mock', 900) RETURNING id",
        )
        .bind(ws)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        Some(Fixture {
            t,
            orch,
            _dir: dir,
            project,
            agent,
        })
    }

    impl Fixture {
        async fn card(&self, title: &str, position: f64) -> Uuid {
            let id = self.t.card(self.project, title).await;
            sqlx::query("UPDATE tasks SET agent_id = $2, position = $3 WHERE id = $1")
                .bind(id)
                .bind(self.agent)
                .bind(position)
                .execute(&self.t.db.pool)
                .await
                .unwrap();
            id
        }

        async fn outcomes(&self) -> Vec<String> {
            sqlx::query_scalar("SELECT outcome FROM heartbeats WHERE agent_id = $1 ORDER BY id")
                .bind(self.agent)
                .fetch_all(&self.t.db.pool)
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn a_beat_starts_the_next_unblocked_card_and_then_waits_for_it() {
        let Some(f) = fixture().await else { return };
        assert_eq!(
            beat(&f.orch, f.agent, Reason::Timer).await,
            Outcome::Idle,
            "nothing yet"
        );

        let blocker = f.t.card(f.project, "elsewhere").await;
        let blocked = f.card("blocked", 0.0).await;
        sqlx::query("INSERT INTO task_deps (task_id, blocked_by) VALUES ($1, $2)")
            .bind(blocked)
            .bind(blocker)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        let second = f.card("second", 2.0).await;
        let first = f.card("first", 1.0).await;

        match beat(&f.orch, f.agent, Reason::Timer).await {
            Outcome::Started { task_id, .. } => {
                assert_eq!(task_id, first, "in order, skipping the blocked one")
            }
            other => panic!("expected a start, got {other:?}"),
        }
        let column: String = sqlx::query_scalar("SELECT board_column FROM tasks WHERE id = $1")
            .bind(first)
            .fetch_one(&f.t.db.pool)
            .await
            .unwrap();
        assert_eq!(column, "running", "started the way the Start button starts");
        assert_eq!(
            beat(&f.orch, f.agent, Reason::Timer).await,
            Outcome::Busy,
            "one at a time"
        );
        let _ = second;
        assert_eq!(f.outcomes().await, ["idle", "started", "busy"]);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_paused_agent_beats_without_starting_anything() {
        let Some(f) = fixture().await else { return };
        f.card("work", 0.0).await;
        sqlx::query("UPDATE agents SET status = 'paused' WHERE id = $1")
            .bind(f.agent)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        assert_eq!(beat(&f.orch, f.agent, Reason::Timer).await, Outcome::Paused);
        let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
            .fetch_one(&f.t.db.pool)
            .await
            .unwrap();
        assert_eq!(runs, 0);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn the_timer_beats_once_per_interval_and_a_wake_beats_early() {
        let Some(f) = fixture().await else { return };
        f.orch.heartbeats().await;
        f.orch.heartbeats().await;
        assert_eq!(f.outcomes().await, ["idle"], "due once, not on every tick");

        // A card handed to it wakes it before its next beat.
        let card = f.card("handed over", 0.0).await;
        wake(&f.t.db, f.agent, "assigned", Some(card)).await;
        wake(&f.t.db, f.agent, "assigned", Some(card)).await;
        wake(&f.t.db, f.agent, "unblocked", Some(card)).await;
        f.orch.heartbeats().await;
        assert_eq!(
            f.outcomes().await,
            ["idle", "started"],
            "three wakes, one beat"
        );
        let reason: String = sqlx::query_scalar(
            "SELECT reason FROM heartbeats WHERE agent_id = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(f.agent)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        assert_eq!(reason, "wake");

        // An agent with no heartbeat is not woken at all.
        sqlx::query("UPDATE agents SET heartbeat_secs = NULL WHERE id = $1")
            .bind(f.agent)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        wake(&f.t.db, f.agent, "assigned", None).await;
        let open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM wakeups WHERE agent_id = $1 AND consumed_at IS NULL",
        )
        .bind(f.agent)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        assert_eq!(open, 0);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_spent_budget_holds_the_start_and_says_why() {
        let Some(f) = fixture().await else { return };
        f.card("work", 0.0).await;
        // A machine-wide cap of one run today, already used.
        sqlx::query(
            "INSERT INTO budget_policies (name, scope_kind, window_kind, on_exceed, cap_runs)
             VALUES ('one a day', 'machine', 'day', 'stop', 1)",
        )
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        let other = f.t.card(f.project, "earlier").await;
        sqlx::query("INSERT INTO runs (task_id, status, trigger, engine, started_at) VALUES ($1, 'completed', 'manual', 'mock', now())")
            .bind(other)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        match beat(&f.orch, f.agent, Reason::Timer).await {
            Outcome::Held { why, .. } => assert!(why.contains("one a day"), "{why}"),
            other => panic!("expected the budget to hold it, got {other:?}"),
        }
        f.t.finish().await;
    }

    /// A live team run for the fixture's project; returns its id.
    async fn team_run(f: &Fixture) -> Uuid {
        let ws: Uuid = sqlx::query_scalar("SELECT workspace_id FROM projects WHERE id = $1")
            .bind(f.project)
            .fetch_one(&f.t.db.pool)
            .await
            .unwrap();
        let team: Uuid = sqlx::query_scalar(
            "INSERT INTO teams (workspace_id, name, pattern, definition) VALUES ($1, 'T', 'org', '{}') RETURNING id",
        )
        .bind(ws)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        sqlx::query_scalar(
            "INSERT INTO runs (team_id, project_id, goal, status, trigger, engine)
             VALUES ($1, $2, 'ship', 'running', 'org', 'mock') RETURNING id",
        )
        .bind(team)
        .bind(f.project)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_step_it_is_doing_for_a_team_keeps_it_busy() {
        let Some(f) = fixture().await else { return };
        f.card("its own", 0.0).await;
        let run = team_run(&f).await;
        // The step names Ada; neither the run nor the team card points at her.
        sqlx::query(
            "INSERT INTO steps (run_id, step_key, status, assignee) VALUES ($1, 'build', 'running', 'Ada')",
        )
        .bind(run)
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        assert_eq!(beat(&f.orch, f.agent, Reason::Timer).await, Outcome::Busy);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_card_that_cannot_start_is_passed_over_not_waited_on() {
        let Some(f) = fixture().await else { return };
        // An orchestrator that also has an engine unable to ask mid-run.
        let mut orch = Orchestrator::new(
            f.t.db.clone(),
            crate::bus::EventBus::new(),
            std::sync::Arc::new(crate::worktrees::manager::WorktreeManager::new(
                f._dir.path().join("wt"),
            )),
            4,
            None,
        );
        orch.register_engine(std::sync::Arc::new(aichip_engines::mock::MockEngine::demo()));
        orch.register_engine(std::sync::Arc::new(
            aichip_engines::gemini::GeminiEngine::default(),
        ));

        // The card's own engine decides, so Ada pins none.
        sqlx::query("UPDATE agents SET engine = NULL WHERE id = $1")
            .bind(f.agent)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        // First in line: Reviewed on Gemini, which no beat will ever start.
        let never = f.card("reviewed on gemini", 0.0).await;
        sqlx::query(
            "UPDATE tasks SET engine = 'gemini', permission_mode = 'reviewed' WHERE id = $1",
        )
        .bind(never)
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        // Second: a sub-task a live team run still holds a step for.
        let held = f.card("a teammate's", 1.0).await;
        let run = team_run(&f).await;
        sqlx::query(
            "INSERT INTO steps (run_id, step_key, status, assignee, task_id) VALUES ($1, 's', 'queued', 'Bo', $2)",
        )
        .bind(run)
        .bind(held)
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        // Third: an ordinary card, which is what the beat starts.
        let next = f.card("ordinary", 2.0).await;
        sqlx::query(
            "UPDATE tasks SET engine = 'mock', permission_mode = 'auto_edit' WHERE id = $1",
        )
        .bind(next)
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        match beat(&orch, f.agent, Reason::Timer).await {
            Outcome::Started { task_id, .. } => assert_eq!(task_id, next),
            other => panic!("expected the ordinary card to start, got {other:?}"),
        }

        // With nothing else left, the beat says which card is stuck and why.
        sqlx::query("UPDATE runs SET status = 'completed' WHERE task_id = $1")
            .bind(next)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE tasks SET board_column = 'done' WHERE id = $1")
            .bind(next)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        match beat(&orch, f.agent, Reason::Timer).await {
            Outcome::Held { why, task_id } => {
                assert_eq!(task_id, Some(never));
                assert!(why.contains("Reviewed"), "{why}");
            }
            other => panic!("expected a hold naming the stuck card, got {other:?}"),
        }
        let logged: Option<Uuid> = sqlx::query_scalar(
            "SELECT task_id FROM heartbeats WHERE agent_id = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(f.agent)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        assert_eq!(logged, Some(never));
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_wake_after_the_heartbeat_was_turned_off_starts_nothing() {
        let Some(f) = fixture().await else { return };
        let card = f.card("handed over", 0.0).await;
        // Woken while the heartbeat was on; turned off before the tick.
        wake(&f.t.db, f.agent, "assigned", Some(card)).await;
        sqlx::query("UPDATE agents SET heartbeat_secs = NULL WHERE id = $1")
            .bind(f.agent)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        f.orch.heartbeats().await;
        let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs")
            .fetch_one(&f.t.db.pool)
            .await
            .unwrap();
        assert_eq!(runs, 0);
        assert_eq!(f.outcomes().await, ["paused"]);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn an_agent_managing_two_projects_takes_their_passes_in_turn() {
        let Some(f) = fixture().await else { return };
        // One manager per project, so a second project for the second pass.
        let (_, other) = f.t.project(&f._dir.path().join("other"), false).await;
        let mut routines = vec![];
        for (name, project) in [("first", f.project), ("second", other)] {
            let ws: Uuid = sqlx::query_scalar("SELECT workspace_id FROM projects WHERE id = $1")
                .bind(project)
                .fetch_one(&f.t.db.pool)
                .await
                .unwrap();
            let id: Uuid = sqlx::query_scalar(
                "INSERT INTO routines (workspace_id, name, kind, project_id, prompt, cron_expr, agent_id, cooldown_secs)
                 VALUES ($1, $2, 'manage', $3, '', '0 9 * * *', $4, 3600) RETURNING id",
            )
            .bind(ws)
            .bind(name)
            .bind(project)
            .bind(f.agent)
            .fetch_one(&f.t.db.pool)
            .await
            .unwrap();
            routines.push(id);
        }
        let passes = |routine: Uuid| {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM routine_runs WHERE routine_id = $1")
                .bind(routine)
                .fetch_one(&f.t.db.pool)
        };
        // The first had a pass two hours ago — past its cooldown, so either
        // may go — and the second has never had one: the beat goes there.
        sqlx::query(
            "INSERT INTO routine_runs (routine_id, trigger, fired_at)
             VALUES ($1, 'heartbeat', now() - interval '2 hours')",
        )
        .bind(routines[0])
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        beat(&f.orch, f.agent, Reason::Timer).await;
        assert_eq!(
            passes(routines[1]).await.unwrap(),
            1,
            "the one waiting longest"
        );
        assert_eq!(passes(routines[0]).await.unwrap(), 1);

        // Now the second is in its cooldown and the first is not.
        sqlx::query(
            "UPDATE routine_runs SET fired_at = now() - interval '3 hours' WHERE routine_id = $1",
        )
        .bind(routines[0])
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        sqlx::query("UPDATE runs SET status = 'completed'")
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE routines SET cooldown_secs = 3600")
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        beat(&f.orch, f.agent, Reason::Timer).await;
        assert_eq!(passes(routines[0]).await.unwrap(), 2, "then the other");
        f.t.finish().await;
    }
}
