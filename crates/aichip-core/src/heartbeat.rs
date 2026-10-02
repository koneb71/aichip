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
    /// Had work, but something said not now (budget, limits, a vet).
    Held(String),
    /// Paused or retired.
    Paused,
}

impl Outcome {
    fn as_str(&self) -> &'static str {
        match self {
            Outcome::Started { .. } => "started",
            Outcome::Fired => "fired",
            Outcome::Idle => "idle",
            Outcome::Busy => "busy",
            Outcome::Held(_) => "held",
            Outcome::Paused => "paused",
        }
    }
}

/// One beat, recorded. Never fails the scheduler: an error is recorded as
/// held.
pub async fn beat(orch: &Orchestrator, agent: Uuid, reason: Reason) -> Outcome {
    let outcome = match try_beat(orch, agent).await {
        Ok(o) => o,
        Err(e) => Outcome::Held(e.to_string()),
    };
    let (task, run, detail) = match &outcome {
        Outcome::Started { task_id, run_id } => (Some(*task_id), Some(*run_id), String::new()),
        Outcome::Held(why) => (None, None, why.chars().take(300).collect()),
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
    let Some(row) = sqlx::query("SELECT status, max_concurrent FROM agents WHERE id = $1")
        .bind(agent)
        .fetch_optional(&orch.db.pool)
        .await?
    else {
        return Ok(Outcome::Paused);
    };
    if row.get::<String, _>("status") != "active" {
        return Ok(Outcome::Paused);
    }
    // One card at a time unless a person allowed more.
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM runs r LEFT JOIN tasks t ON t.id = r.task_id
          WHERE COALESCE(r.agent_id, t.agent_id) = $1
            AND r.status IN ('queued', 'starting', 'running', 'waiting_permission', 'rate_limited')",
    )
    .bind(agent)
    .fetch_one(&orch.db.pool)
    .await?;
    let allowed = row
        .get::<Option<i32>, _>("max_concurrent")
        .unwrap_or(1)
        .max(1) as i64;
    if live >= allowed {
        return Ok(Outcome::Busy);
    }

    // A manager agent's job is its pass.
    let routine: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM routines WHERE agent_id = $1 AND kind = 'manage' AND enabled LIMIT 1",
    )
    .bind(agent)
    .fetch_optional(&orch.db.pool)
    .await?;
    if let Some(routine) = routine {
        if !orch.may_wake_manager(routine).await? {
            return Ok(Outcome::Busy);
        }
        crate::routines::fire(&orch.db, orch, routine, "heartbeat").await?;
        return Ok(Outcome::Fired);
    }

    // Otherwise its next card: assigned to it, in the backlog, and with
    // nothing it waits on still unlanded.
    let next: Option<Uuid> = sqlx::query_scalar(
        "SELECT t.id FROM tasks t
          WHERE t.agent_id = $1 AND t.team_id IS NULL AND t.board_column = 'backlog'
            AND NOT EXISTS (SELECT 1 FROM task_deps d JOIN tasks b ON b.id = d.blocked_by
                             WHERE d.task_id = t.id AND b.board_column <> 'done')
            AND NOT EXISTS (SELECT 1 FROM runs r WHERE r.task_id = t.id
                               AND r.status NOT IN ('completed', 'failed', 'canceled'))
          ORDER BY t.position, t.created_at LIMIT 1",
    )
    .bind(agent)
    .fetch_optional(&orch.db.pool)
    .await?;
    let Some(task_id) = next else {
        return Ok(Outcome::Idle);
    };
    match orch.start_card(task_id).await {
        Ok(run_id) => Ok(Outcome::Started { task_id, run_id }),
        Err(e) => Ok(Outcome::Held(e.to_string())),
    }
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
            Outcome::Held(why) => assert!(why.contains("one a day"), "{why}"),
            other => panic!("expected the budget to hold it, got {other:?}"),
        }
        f.t.finish().await;
    }
}
