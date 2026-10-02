//! Wakes: news a project's manager would want before its next scheduled pass.
//!
//! A manager routine runs on a schedule, so a card that fails at 9:05 waits
//! for the 9:00-tomorrow pass to be noticed. A wake is a row saying what
//! happened; the scheduler drains them and fires the manager early — but only
//! for the kinds a person ticked (`routines.on_events`), only when its thread
//! is idle (a busy thread keeps them for later rather than dropping them),
//! after a cooldown, and up to a number of passes a day. Every pass, early or
//! scheduled, opens with what happened since the last one and consumes it.
//!
//! Raising one never fails the thing that raised it, and never starts
//! anything itself: the only door it opens is `routines::fire`, whose gates —
//! the agent, the budget, the thread — all still apply.

use crate::db::Db;
use crate::runs::orchestrator::Orchestrator;
use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A card reached done.
    Landed,
    /// A card's blocker landed and it can start.
    Unblocked,
    /// A card's work run failed.
    Failed,
    /// A card's checks still fail after the fixes the project allows.
    ChecksExhausted,
    /// A card's agent review stopped for a person.
    ReviewExhausted,
    /// A card's agent asked a person something.
    Question,
    /// A run stopped showing signs of life and was stopped.
    Stalled,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Landed,
        Kind::Unblocked,
        Kind::Failed,
        Kind::ChecksExhausted,
        Kind::ReviewExhausted,
        Kind::Question,
        Kind::Stalled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Landed => "landed",
            Kind::Unblocked => "unblocked",
            Kind::Failed => "failed",
            Kind::ChecksExhausted => "checks_exhausted",
            Kind::ReviewExhausted => "review_exhausted",
            Kind::Question => "question",
            Kind::Stalled => "stalled",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// How the pass prompt says it.
    fn says(self) -> &'static str {
        match self {
            Kind::Landed => "landed",
            Kind::Unblocked => "can start — what blocked it landed",
            Kind::Failed => "failed",
            Kind::ChecksExhausted => "still fails its checks after the allowed fixes",
            Kind::ReviewExhausted => "ran out of review rounds and waits for a person",
            Kind::Question => "is waiting for a person to answer its agent's question",
            Kind::Stalled => "stalled and was stopped",
        }
    }
}

/// Keep only the kinds this version knows, in a stable order. What a manager
/// is woken for is stored as text, so an unknown one is dropped, not trusted.
pub fn known(events: &[String]) -> Vec<String> {
    Kind::ALL
        .into_iter()
        .filter(|k| events.iter().any(|e| e == k.as_str()))
        .map(|k| k.as_str().to_string())
        .collect()
}

/// Say that something happened to a card. Wakes the card's project's manager
/// if it asked to hear about this kind; a no-op otherwise. Best-effort.
pub async fn raise(db: &Db, task_id: Uuid, run_id: Option<Uuid>, kind: Kind, detail: &str) {
    if let Err(e) = try_raise(db, task_id, run_id, kind, detail).await {
        tracing::warn!(%task_id, kind = kind.as_str(), error = %e, "could not raise a wake");
    }
}

async fn try_raise(
    db: &Db,
    task_id: Uuid,
    run_id: Option<Uuid>,
    kind: Kind,
    detail: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO wakeups (routine_id, kind, task_id, run_id, detail)
         SELECT rt.id, $2, t.id, $3, $4
           FROM tasks t JOIN routines rt ON rt.project_id = t.project_id
          WHERE t.id = $1 AND rt.kind = 'manage' AND rt.enabled AND $2 = ANY(rt.on_events)
         ON CONFLICT (routine_id, kind, COALESCE(task_id, '00000000-0000-0000-0000-000000000000'::uuid))
            WHERE consumed_at IS NULL AND routine_id IS NOT NULL
         DO UPDATE SET count = wakeups.count + 1, last_at = now(),
                       run_id = EXCLUDED.run_id, detail = EXCLUDED.detail",
    )
    .bind(task_id)
    .bind(kind.as_str())
    .bind(run_id)
    .bind(detail.chars().take(300).collect::<String>())
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// One wake, as a pass reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Wake {
    pub id: Uuid,
    pub kind: Kind,
    pub card: Option<String>,
    pub detail: String,
    pub count: i32,
    pub last_at: DateTime<Utc>,
}

/// Every unread wake for a routine, oldest first.
pub async fn pending(db: &Db, routine_id: Uuid) -> anyhow::Result<Vec<Wake>> {
    let rows = sqlx::query(
        "SELECT w.id, w.kind, t.title, w.detail, w.count, w.last_at
           FROM wakeups w LEFT JOIN tasks t ON t.id = w.task_id
          WHERE w.routine_id = $1 AND w.consumed_at IS NULL
          ORDER BY w.created_at LIMIT 50",
    )
    .bind(routine_id)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(Wake {
                id: r.get("id"),
                kind: Kind::parse(&r.get::<String, _>("kind"))?,
                card: r.get("title"),
                detail: r.get("detail"),
                count: r.get("count"),
                last_at: r.get("last_at"),
            })
        })
        .collect())
}

/// Mark wakes read by the pass that read them.
pub async fn consume(db: &Db, ids: &[Uuid], pass_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE wakeups SET consumed_at = now(), routine_run_id = $2
          WHERE id = ANY($1) AND consumed_at IS NULL",
    )
    .bind(ids)
    .bind(pass_id)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// What happened since the last pass, for the top of the next one. Pure, for
/// the tests. Card titles are written by people and agents alike, so the
/// list is fenced: it is news to act on, not instructions.
pub fn render(wakes: &[Wake]) -> Option<String> {
    if wakes.is_empty() {
        return None;
    }
    let lines: Vec<String> = wakes
        .iter()
        .map(|w| {
            let card = match &w.card {
                Some(t) => format!("\u{201c}{t}\u{201d}"),
                None => "A card".to_string(),
            };
            let times = if w.count > 1 {
                format!(" ({}×)", w.count)
            } else {
                String::new()
            };
            let detail = if w.detail.trim().is_empty() {
                String::new()
            } else {
                format!(" — {}", w.detail.trim())
            };
            format!("- {card} {}{times}{detail}", w.kind.says())
        })
        .collect();
    Some(format!(
        "## Since your last pass\n\nWhat happened on this board that you asked to hear about. \
         Start with these.\n\n{}",
        crate::fence::wrap(
            crate::fence::WAKE_BEGIN,
            crate::fence::WAKE_END,
            &lines.join("\n")
        )
    ))
}

/// Whether a routine may be woken now. Pure, for the tests: the thread is
/// idle, the cooldown since its last pass has passed, and it has passes left
/// today.
pub fn may_wake(
    busy: bool,
    last_pass: Option<DateTime<Utc>>,
    passes_today: i64,
    cooldown_secs: i64,
    max_per_day: i64,
    now: DateTime<Utc>,
) -> bool {
    !busy
        && passes_today < max_per_day
        && last_pass.is_none_or(|at| (now - at).num_seconds() >= cooldown_secs)
}

impl Orchestrator {
    /// Fire every manager with news waiting that may be woken now. Each
    /// scheduler tick. A manager that may not be woken keeps its wakes —
    /// they coalesce, and the next pass, early or scheduled, reads them.
    pub async fn drain_wakeups(&self) {
        let due: Vec<Uuid> = match sqlx::query_scalar(
            "SELECT DISTINCT w.routine_id FROM wakeups w
               JOIN routines rt ON rt.id = w.routine_id
              WHERE w.consumed_at IS NULL AND rt.enabled AND rt.kind = 'manage'
                AND w.kind = ANY(rt.on_events)",
        )
        .fetch_all(&self.db.pool)
        .await
        {
            Ok(due) => due,
            Err(e) => {
                tracing::warn!(error = %e, "could not read wakes");
                return;
            }
        };
        for routine_id in due {
            match self.may_wake_routine(routine_id).await {
                Ok(true) => {
                    if let Err(e) = crate::routines::fire(&self.db, self, routine_id, "wake").await
                    {
                        tracing::info!(%routine_id, error = %e, "a wake did not fire its manager");
                    }
                }
                Ok(false) => {}
                Err(e) => tracing::warn!(%routine_id, error = %e, "could not check a wake"),
            }
        }
    }

    async fn may_wake_routine(&self, routine_id: Uuid) -> anyhow::Result<bool> {
        let r = sqlx::query(
            "SELECT rt.cooldown_secs, rt.max_passes_per_day,
                    EXISTS (SELECT 1 FROM runs WHERE chat_id = rt.chat_id
                               AND status NOT IN ('completed','failed','canceled')) AS busy,
                    (SELECT max(rr.fired_at) FROM routine_runs rr WHERE rr.routine_id = rt.id) AS last_pass,
                    (SELECT count(*) FROM routine_runs rr WHERE rr.routine_id = rt.id
                        AND rr.trigger = 'wake'
                        AND rr.fired_at >= date_trunc('day', now())) AS passes_today
               FROM routines rt WHERE rt.id = $1",
        )
        .bind(routine_id)
        .fetch_one(&self.db.pool)
        .await?;
        Ok(may_wake(
            r.get("busy"),
            r.get("last_pass"),
            r.get("passes_today"),
            r.get::<i32, _>("cooldown_secs") as i64,
            r.get::<i32, _>("max_passes_per_day") as i64,
            Utc::now(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wake(kind: Kind, card: Option<&str>, count: i32) -> Wake {
        Wake {
            id: Uuid::nil(),
            kind,
            card: card.map(str::to_string),
            detail: String::new(),
            count,
            last_at: Utc::now(),
        }
    }

    #[test]
    fn kinds_round_trip_and_unknown_ones_are_dropped() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.as_str()), Some(k));
        }
        assert_eq!(
            known(&["failed".into(), "rm -rf".into(), "landed".into()]),
            ["landed", "failed"]
        );
    }

    #[test]
    fn a_pass_reads_what_happened_fenced_and_counted() {
        assert_eq!(render(&[]), None);
        let text = render(&[
            wake(Kind::Failed, Some("Fix login"), 3),
            wake(Kind::Landed, Some("Add flag"), 1),
        ])
        .unwrap();
        assert!(text.contains("- \u{201c}Fix login\u{201d} failed (3×)"));
        assert!(text.contains("- \u{201c}Add flag\u{201d} landed"));
        assert_eq!(text.matches(crate::fence::WAKE_BEGIN).count(), 1);
        // A title cannot close the fence early.
        let sneaky = render(&[wake(Kind::Landed, Some(crate::fence::WAKE_END), 1)]).unwrap();
        assert_eq!(sneaky.matches(crate::fence::WAKE_END).count(), 1);
    }

    #[test]
    fn a_wake_waits_for_an_idle_thread_the_cooldown_and_the_daily_cap() {
        let now = Utc::now();
        let ago = |s: i64| Some(now - chrono::Duration::seconds(s));
        assert!(may_wake(false, None, 0, 900, 6, now));
        assert!(!may_wake(true, None, 0, 900, 6, now), "busy thread");
        assert!(!may_wake(false, ago(60), 0, 900, 6, now), "cooling down");
        assert!(may_wake(false, ago(900), 0, 900, 6, now));
        assert!(
            !may_wake(false, ago(9000), 6, 900, 6, now),
            "spent for today"
        );
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
        card: Uuid,
        routine: Uuid,
    }

    /// A project with a manager that asked to hear about failures only. The
    /// queue is paused, so a pass that fires stays queued.
    async fn fixture() -> Option<Fixture> {
        let t = testdb::fresh().await?;
        let dir = tempfile::tempdir().unwrap();
        let orch = t.orchestrator(dir.path());
        orch.set_queue_paused(true).await.unwrap();
        let (ws, project) = t.project(dir.path(), false).await;
        let card = t.card(project, "Fix login").await;
        let routine: Uuid = sqlx::query_scalar(
            "INSERT INTO routines (workspace_id, name, kind, project_id, prompt, cron_expr, engine, on_events)
             VALUES ($1, 'manager', 'manage', $2, 'keep it moving', '0 9 * * *', 'mock', '{failed}')
             RETURNING id",
        )
        .bind(ws)
        .bind(project)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        Some(Fixture {
            t,
            orch,
            _dir: dir,
            card,
            routine,
        })
    }

    impl Fixture {
        async fn open(&self) -> Vec<(String, i32)> {
            sqlx::query_as(
                "SELECT kind, count FROM wakeups WHERE routine_id = $1 AND consumed_at IS NULL",
            )
            .bind(self.routine)
            .fetch_all(&self.t.db.pool)
            .await
            .unwrap()
        }

        async fn passes(&self) -> i64 {
            sqlx::query_scalar(
                "SELECT count(*) FROM routine_runs WHERE routine_id = $1 AND trigger = 'wake'",
            )
            .bind(self.routine)
            .fetch_one(&self.t.db.pool)
            .await
            .unwrap()
        }
    }

    #[tokio::test]
    async fn the_same_news_coalesces_and_unasked_news_is_not_kept() {
        let Some(f) = fixture().await else { return };
        raise(&f.t.db, f.card, None, Kind::Failed, "exit 1").await;
        raise(&f.t.db, f.card, None, Kind::Failed, "exit 2").await;
        raise(&f.t.db, f.card, None, Kind::Landed, "").await;
        assert_eq!(f.open().await, [("failed".to_string(), 2)]);
        // A card's run failing raises it on its own, through `finish`.
        let run: Uuid = sqlx::query_scalar(
            "INSERT INTO runs (task_id, status, trigger, engine) VALUES ($1, 'running', 'manual', 'mock') RETURNING id",
        )
        .bind(f.card)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        f.orch
            .finish(run, aichip_shared::RunStatus::Failed, Some("exit 3".into()))
            .await
            .unwrap();
        assert_eq!(f.open().await, [("failed".to_string(), 3)]);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_busy_thread_keeps_its_news_and_an_idle_one_reads_it_once() {
        let Some(f) = fixture().await else { return };
        // The manager's thread has a turn going.
        let chat: Uuid = sqlx::query_scalar(
            "INSERT INTO chats (title, project_id)
             SELECT 'manager', project_id FROM routines WHERE id = $1 RETURNING id",
        )
        .bind(f.routine)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        sqlx::query("UPDATE routines SET chat_id = $2 WHERE id = $1")
            .bind(f.routine)
            .bind(chat)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        let busy: Uuid = sqlx::query_scalar(
            "INSERT INTO runs (chat_id, status, trigger, engine) VALUES ($1, 'running', 'chat', 'mock') RETURNING id",
        )
        .bind(chat)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        raise(&f.t.db, f.card, None, Kind::Failed, "exit 1").await;
        f.orch.drain_wakeups().await;
        assert_eq!(f.passes().await, 0, "deferred, not fired");
        assert_eq!(f.open().await.len(), 1, "and not dropped");

        sqlx::query("UPDATE runs SET status = 'completed' WHERE id = $1")
            .bind(busy)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        f.orch.drain_wakeups().await;
        assert_eq!(f.passes().await, 1);
        assert!(f.open().await.is_empty(), "read by the pass that fired");
        let asked: String = sqlx::query_scalar(
            "SELECT content FROM chat_messages WHERE chat_id = $1 AND role = 'user'
              ORDER BY created_at DESC LIMIT 1",
        )
        .bind(chat)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        assert!(
            asked.contains("Since your last pass") && asked.contains("Fix login"),
            "{asked}"
        );

        // More news straight away: the cooldown holds it for later.
        raise(&f.t.db, f.card, None, Kind::Failed, "exit 3").await;
        sqlx::query("UPDATE runs SET status = 'completed' WHERE chat_id = $1")
            .bind(chat)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        f.orch.drain_wakeups().await;
        assert_eq!(f.passes().await, 1, "cooling down");
        assert_eq!(f.open().await.len(), 1);
        f.t.finish().await;
    }
}
