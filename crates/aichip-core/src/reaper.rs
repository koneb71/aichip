//! Runs that stop showing signs of life.
//!
//! Two ways a run can be stuck, and they are told apart by different things:
//!
//! - **Lost.** The row says `starting` or `running`, but nothing in this
//!   process is executing it — its task died (a panic, a dropped future) and
//!   nobody will ever write its ending. `recover_orphans` catches this across
//!   a restart; this catches it while the server keeps running. Always on:
//!   a run nothing is executing cannot finish by itself.
//! - **Silent.** It is executing, but has not said anything for longer than
//!   the person allows. Off by default — a long build or test run is silent
//!   too — and never for a run waiting on a person's permission.
//!
//! Either is said on the card, raised as a `stalled` wake for the manager and
//! sent through attention. Opt-in, the run is resumed: only for these two
//! reasons, at most twice along a `resumed_from` chain, and through
//! `runs::resume::resume_dead_run` — the Resume button's own door — so the
//! agent, the budget and the worktree are vetted as for a click.

use crate::db::Db;
use crate::runs::orchestrator::Orchestrator;
use aichip_shared::RunStatus;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

/// A run nothing executes is only called lost after this long in the row's
/// state, so a run between its claim and its first event is never mistaken
/// for one.
pub const LOST_AFTER: Duration = Duration::from_secs(120);

/// Resumes aichip makes along one chain before leaving it to a person.
pub const MAX_AUTO_RESUMES: i64 = 2;

pub const MAX_SILENCE_MINUTES: i64 = 240;

/// The "Unattended runs" setting.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Unattended {
    /// Stop a run that has said nothing for this long. 0 is off.
    pub silence_minutes: i64,
    /// Pick a lost or silenced run back up by itself.
    pub auto_resume: bool,
}

impl Default for Unattended {
    fn default() -> Self {
        Self {
            silence_minutes: 0,
            auto_resume: false,
        }
    }
}

impl Unattended {
    pub fn clamped(self) -> Self {
        Self {
            silence_minutes: self.silence_minutes.clamp(0, MAX_SILENCE_MINUTES),
            ..self
        }
    }
}

pub async fn load(db: &Db) -> Unattended {
    let stored: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'unattended'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    stored
        .and_then(|v| serde_json::from_value::<Unattended>(v).ok())
        .unwrap_or_default()
        .clamped()
}

pub async fn save(db: &Db, u: Unattended) -> anyhow::Result<Unattended> {
    let u = u.clamped();
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('unattended', $1)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(serde_json::to_value(u)?)
    .execute(&db.pool)
    .await?;
    Ok(u)
}

/// Why a run was stopped, as the card says it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stall {
    Lost,
    Silent(i64),
}

impl Stall {
    pub fn as_str(self) -> &'static str {
        match self {
            Stall::Lost => "lost",
            Stall::Silent(_) => "silent",
        }
    }

    pub fn sentence(self) -> String {
        match self {
            Stall::Lost => "The process working on this run is gone, so it was marked failed. Resume picks it up where it stopped.".into(),
            Stall::Silent(m) => format!(
                "This run said nothing for {m} minutes, so it was stopped. Resume picks it up where it stopped."
            ),
        }
    }
}

/// Is a run silent past the limit? Pure, for the tests.
pub fn is_silent(since_last_sign: Duration, silence_minutes: i64) -> bool {
    silence_minutes > 0 && since_last_sign >= Duration::from_secs(silence_minutes as u64 * 60)
}

impl Orchestrator {
    /// One sweep. Each scheduler tick.
    pub async fn reap(&self) {
        let settings = load(&self.db).await;
        if let Err(e) = self.reap_lost().await {
            tracing::warn!(error = %e, "could not look for lost runs");
        }
        if settings.silence_minutes > 0 {
            if let Err(e) = self.reap_silent(settings.silence_minutes).await {
                tracing::warn!(error = %e, "could not look for silent runs");
            }
        }
        if settings.auto_resume {
            if let Err(e) = self.resume_reaped().await {
                tracing::warn!(error = %e, "could not resume stopped runs");
            }
        }
    }

    async fn reap_lost(&self) -> anyhow::Result<()> {
        let candidates: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM runs WHERE status IN ('starting', 'running', 'waiting_permission')
                AND COALESCE(started_at, created_at) < now() - make_interval(secs => $1)",
        )
        .bind(LOST_AFTER.as_secs_f64())
        .fetch_all(&self.db.pool)
        .await?;
        for run_id in candidates {
            if self.is_executing(run_id) {
                continue;
            }
            // Marked first, and only while still in that state: a run that
            // ended between the read and now is left alone.
            let marked = sqlx::query(
                "UPDATE runs SET reaped = 'lost'
                  WHERE id = $1 AND status IN ('starting', 'running', 'waiting_permission')
                    AND reaped IS NULL",
            )
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
            if marked.rows_affected() == 0 || self.is_executing(run_id) {
                continue;
            }
            self.finish(
                run_id,
                RunStatus::Failed,
                Some("process lost — nothing was running it any more".into()),
            )
            .await?;
            self.stalled(run_id, Stall::Lost).await;
        }
        Ok(())
    }

    async fn reap_silent(&self, minutes: i64) -> anyhow::Result<()> {
        let running: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM runs WHERE status = 'running' AND reaped IS NULL")
                .fetch_all(&self.db.pool)
                .await?;
        for run_id in running {
            let Some(since) = self.since_last_sign(run_id) else {
                continue;
            };
            if !is_silent(since, minutes) {
                continue;
            }
            let reason = format!("no output for {minutes} minutes");
            // The reason first: the cancel ends through `finish`, which keeps
            // a reason already written rather than overwriting it with none.
            let marked = sqlx::query(
                "UPDATE runs SET reaped = 'silent', error_reason = $2
                  WHERE id = $1 AND status = 'running' AND reaped IS NULL",
            )
            .bind(run_id)
            .bind(&reason)
            .execute(&self.db.pool)
            .await?;
            if marked.rows_affected() == 0 {
                continue;
            }
            self.cancel(run_id);
            self.stalled(run_id, Stall::Silent(minutes)).await;
        }
        Ok(())
    }

    /// Resume what this sweep (or an earlier one) stopped, once each.
    async fn resume_reaped(&self) -> anyhow::Result<()> {
        let due: Vec<Uuid> = sqlx::query_scalar(
            "SELECT r.id FROM runs r
              WHERE r.reaped IS NOT NULL AND r.auto_resumed_at IS NULL
                AND r.status IN ('failed', 'canceled')
                AND r.finished_at > now() - interval '1 hour'
                AND NOT EXISTS (SELECT 1 FROM runs n WHERE n.resumed_from = r.id)",
        )
        .fetch_all(&self.db.pool)
        .await?;
        for run_id in due {
            if self.is_executing(run_id) {
                continue;
            }
            let claimed = sqlx::query(
                "UPDATE runs SET auto_resumed_at = now() WHERE id = $1 AND auto_resumed_at IS NULL",
            )
            .bind(run_id)
            .execute(&self.db.pool)
            .await?;
            if claimed.rows_affected() == 0 {
                continue;
            }
            let task_id: Option<Uuid> =
                sqlx::query_scalar("SELECT task_id FROM runs WHERE id = $1")
                    .bind(run_id)
                    .fetch_one(&self.db.pool)
                    .await?;
            let Some(task_id) = task_id else { continue };
            let depth = chain_depth(&self.db, run_id).await?;
            let said = if depth >= MAX_AUTO_RESUMES {
                format!("Not resumed again: it has already been picked back up {depth} times. Over to you.")
            } else {
                match crate::runs::resume::resume_dead_run(self, run_id).await {
                    Ok((new_run, _)) => {
                        crate::audit::record(
                            &self.db,
                            crate::audit::Entry::new(
                                crate::audit::Actor::System,
                                "resumed a stopped run",
                            )
                            .on("runs", run_id)
                            .detail(serde_json::json!({ "resumedAs": new_run })),
                        )
                        .await;
                        "Resumed it where it stopped.".to_string()
                    }
                    Err(e) => format!("Could not resume it: {e}"),
                }
            };
            crate::runs::report::post_system(&self.db, task_id, Some(run_id), &said).await?;
        }
        Ok(())
    }

    /// Said on the card, raised for the manager, sent through attention,
    /// written to the ledger. Best-effort.
    async fn stalled(&self, run_id: Uuid, why: Stall) {
        crate::audit::record(
            &self.db,
            crate::audit::Entry::new(crate::audit::Actor::System, "stopped a stalled run")
                .on("runs", run_id)
                .summary(why.as_str()),
        )
        .await;
        let task: Option<Uuid> = sqlx::query_scalar("SELECT task_id FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_optional(&self.db.pool)
            .await
            .ok()
            .flatten()
            .flatten();
        if let Some(task_id) = task {
            if let Err(e) =
                crate::runs::report::post_system(&self.db, task_id, Some(run_id), &why.sentence())
                    .await
            {
                tracing::warn!(%run_id, error = %e, "could not note a stalled run");
            }
            crate::wake::raise(
                &self.db,
                task_id,
                Some(run_id),
                crate::wake::Kind::Stalled,
                why.as_str(),
            )
            .await;
        }
        let ctx = crate::attention::Ctx {
            title: "aichip: a run stalled".into(),
            body: why.sentence(),
            ..crate::attention::ctx_for_run(&self.db, run_id, None).await
        };
        crate::attention::fire(&self.db, crate::attention::Event::Stalled, ctx).await;
    }
}

/// How many resumes led to this run.
async fn chain_depth(db: &Db, run_id: Uuid) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar(
        "WITH RECURSIVE chain(id, prior, depth) AS (
             SELECT id, resumed_from, 0 FROM runs WHERE id = $1
             UNION ALL
             SELECT r.id, r.resumed_from, c.depth + 1
               FROM runs r JOIN chain c ON r.id = c.prior
              WHERE c.depth < 10)
         SELECT COALESCE(max(depth), 0)::bigint FROM chain",
    )
    .bind(run_id)
    .fetch_one(&db.pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_off_by_default_and_measured_in_minutes() {
        assert!(!is_silent(Duration::from_secs(10_000), 0), "off");
        assert!(!is_silent(Duration::from_secs(599), 10));
        assert!(is_silent(Duration::from_secs(600), 10));
        assert_eq!(
            Unattended {
                silence_minutes: 99_999,
                auto_resume: true
            }
            .clamped()
            .silence_minutes,
            MAX_SILENCE_MINUTES
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
    }

    async fn fixture() -> Option<Fixture> {
        let t = testdb::fresh().await?;
        let dir = tempfile::tempdir().unwrap();
        let orch = t.orchestrator(dir.path());
        orch.set_queue_paused(true).await.unwrap();
        let (_, project) = t.project(dir.path(), false).await;
        let card = t.card(project, "long job").await;
        sqlx::query("UPDATE tasks SET board_column = 'running', worktree_path = $2 WHERE id = $1")
            .bind(card)
            .bind(dir.path().to_string_lossy().as_ref())
            .execute(&t.db.pool)
            .await
            .unwrap();
        Some(Fixture {
            t,
            orch,
            _dir: dir,
            card,
        })
    }

    impl Fixture {
        /// A run of the card in `status`, started `mins` minutes ago.
        async fn run(&self, status: &str, mins: i32) -> Uuid {
            sqlx::query_scalar(
                "INSERT INTO runs (task_id, status, trigger, engine, session_id, session_engine, started_at)
                 VALUES ($1, $2, 'manual', 'mock', 'sess-1', 'mock', now() - make_interval(mins => $3))
                 RETURNING id",
            )
            .bind(self.card)
            .bind(status)
            .bind(mins)
            .fetch_one(&self.t.db.pool)
            .await
            .unwrap()
        }

        async fn state(&self, run: Uuid) -> (String, Option<String>, Option<String>) {
            sqlx::query_as("SELECT status, reaped, error_reason FROM runs WHERE id = $1")
                .bind(run)
                .fetch_one(&self.t.db.pool)
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn a_run_nothing_is_executing_is_lost_and_one_still_executing_is_not() {
        let Some(f) = fixture().await else { return };
        let lost = f.run("running", 5).await;
        let alive_run = f.run("running", 5).await;
        let fresh = f.run("starting", 0).await;
        let _alive = f.orch.alive(alive_run);
        f.orch.reap().await;
        let (status, reaped, _) = f.state(lost).await;
        assert_eq!(
            (status.as_str(), reaped.as_deref()),
            ("failed", Some("lost"))
        );
        assert_eq!(f.state(alive_run).await.0, "running", "still executing");
        assert_eq!(
            f.state(fresh).await.0,
            "starting",
            "just claimed — not lost yet"
        );
        let noted: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM task_comments WHERE task_id = $1 AND content LIKE 'The process working%'",
        )
        .bind(f.card)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        assert_eq!(noted, 1);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn silence_stops_a_run_only_when_a_person_set_a_limit() {
        let Some(f) = fixture().await else { return };
        let quiet = f.run("running", 30).await;
        let _alive = f.orch.alive(quiet);
        f.orch.backdate(quiet, Duration::from_secs(20 * 60));
        f.orch.reap().await;
        assert_eq!(f.state(quiet).await.1, None, "off by default");

        save(
            &f.t.db,
            Unattended {
                silence_minutes: 15,
                auto_resume: false,
            },
        )
        .await
        .unwrap();
        f.orch.reap().await;
        let (_, reaped, reason) = f.state(quiet).await;
        assert_eq!(reaped.as_deref(), Some("silent"));
        assert_eq!(reason.as_deref(), Some("no output for 15 minutes"));

        // Waiting on a person is not silence.
        let waiting = f.run("waiting_permission", 30).await;
        let _w = f.orch.alive(waiting);
        f.orch.backdate(waiting, Duration::from_secs(60 * 60));
        f.orch.reap().await;
        assert_eq!(f.state(waiting).await.1, None);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn auto_resume_picks_a_stopped_run_up_once_and_not_forever() {
        let Some(f) = fixture().await else { return };
        save(
            &f.t.db,
            Unattended {
                silence_minutes: 0,
                auto_resume: true,
            },
        )
        .await
        .unwrap();
        let lost = f.run("running", 5).await;
        f.orch.reap().await;
        // Lost this tick; resumed the same tick, through the Resume door.
        let resumed: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM runs WHERE resumed_from = $1")
            .bind(lost)
            .fetch_all(&f.t.db.pool)
            .await
            .unwrap();
        assert_eq!(resumed.len(), 1);
        f.orch.reap().await;
        let again: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE resumed_from = $1")
            .bind(lost)
            .fetch_one(&f.t.db.pool)
            .await
            .unwrap();
        assert_eq!(again, 1, "once");

        // A chain already resumed twice (root → lost → deep) is left to a
        // person.
        let deep = resumed[0];
        sqlx::query("UPDATE runs SET status = 'failed', reaped = 'lost', finished_at = now(), resumed_from = $2 WHERE id = $1")
            .bind(deep)
            .bind(lost)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        let root = f.run("failed", 60).await;
        sqlx::query("UPDATE runs SET resumed_from = $2 WHERE id = $1")
            .bind(lost)
            .bind(root)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        f.orch.reap().await;
        let past: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE resumed_from = $1")
            .bind(deep)
            .fetch_one(&f.t.db.pool)
            .await
            .unwrap();
        assert_eq!(past, 0, "two resumes deep: over to you");
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_refused_resume_is_said_once_not_every_tick() {
        let Some(f) = fixture().await else { return };
        save(
            &f.t.db,
            Unattended {
                silence_minutes: 0,
                auto_resume: true,
            },
        )
        .await
        .unwrap();
        // Its worktree is gone, so the Resume door refuses it.
        sqlx::query("UPDATE tasks SET worktree_path = '/nonexistent/aichip' WHERE id = $1")
            .bind(f.card)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        f.run("running", 5).await;
        f.orch.reap().await;
        f.orch.reap().await;
        f.orch.reap().await;
        let said: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM task_comments WHERE task_id = $1 AND content LIKE 'Could not resume%'",
        )
        .bind(f.card)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        assert_eq!(said, 1);
        f.t.finish().await;
    }
}
