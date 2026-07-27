//! Cron scheduling for workflows.
//!
//! The scheduler only ever *enqueues* — it never spawns an engine itself —
//! so concurrency limits, rate-limit backoff, and cancellation all behave
//! exactly as they do for a manual run.

use chrono::{DateTime, Duration, Utc};
use croner::Cron;
use sqlx::Row;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::Db;
use crate::runs::orchestrator::Orchestrator;

/// How often the loop wakes up. Cron granularity is a minute, so this is
/// comfortably fine-grained.
const TICK: std::time::Duration = std::time::Duration::from_secs(30);

/// A fire that is overdue by more than this is treated as "missed" — the
/// machine was probably asleep or the server was down.
pub const GRACE: Duration = Duration::minutes(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchUp {
    /// Don't run work that was missed while the server was down (default).
    Skip,
    /// Run a single catch-up execution for the missed window.
    RunOnce,
}

impl From<&str> for CatchUp {
    fn from(s: &str) -> Self {
        match s {
            "run_once" => CatchUp::RunOnce,
            _ => CatchUp::Skip,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Due now — enqueue a run.
    Fire,
    /// Due, but so long ago that it should be written off; advance the
    /// bookmark without running anything.
    SkipMissed,
    /// Never scheduled before: start the clock, don't run anything yet.
    Bookmark,
    /// Not due yet.
    Wait,
}

/// Decide what to do with one scheduled workflow. Pure, so the interesting
/// cases are unit-testable without a database or a clock.
pub fn decide(
    cron: &Cron,
    last_fired: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    catch_up: CatchUp,
    grace: Duration,
) -> Decision {
    // With no history, schedule from now: a workflow saved at 14:00 with a
    // 03:00 daily cron shouldn't immediately fire for this morning.
    let Some(since) = last_fired else {
        return Decision::Bookmark;
    };
    let Ok(next) = cron.find_next_occurrence(&since, false) else {
        return Decision::Wait;
    };
    if next > now {
        return Decision::Wait;
    }
    if catch_up == CatchUp::RunOnce || now - next <= grace {
        Decision::Fire
    } else {
        Decision::SkipMissed
    }
}

pub struct Scheduler {
    db: Db,
    orchestrator: Arc<Orchestrator>,
}

impl Scheduler {
    pub fn new(db: Db, orchestrator: Arc<Orchestrator>) -> Self {
        Self { db, orchestrator }
    }

    pub async fn run_loop(self) {
        loop {
            if let Err(e) = self.tick().await {
                tracing::error!(error=%e, "scheduler tick failed");
            }
            tokio::time::sleep(TICK).await;
        }
    }

    async fn tick(&self) -> anyhow::Result<()> {
        let rows = sqlx::query(
            "SELECT id, name, cron_expr, last_fired_at, catch_up FROM workflows
             WHERE enabled AND cron_expr IS NOT NULL",
        )
        .fetch_all(&self.db.pool)
        .await?;

        let now = Utc::now();
        for row in rows {
            let workflow_id: Uuid = row.get("id");
            let expr: String = row.get("cron_expr");
            let cron = match Cron::from_str(&expr) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(%workflow_id, %expr, error=%e, "invalid cron expression");
                    continue;
                }
            };
            let catch_up = CatchUp::from(row.get::<String, _>("catch_up").as_str());
            let decision = decide(
                &cron,
                row.get("last_fired_at"),
                now,
                catch_up,
                GRACE,
            );

            match decision {
                Decision::Wait => continue,
                // First sighting (new workflow, or one just re-enabled):
                // start its clock now so the next occurrence is measured
                // from here rather than firing retroactively.
                Decision::Bookmark => {
                    tracing::debug!(
                        workflow = %row.get::<String, _>("name"),
                        "scheduling from now"
                    );
                }
                Decision::SkipMissed => {
                    tracing::info!(
                        workflow = %row.get::<String, _>("name"),
                        "skipping a missed schedule (server was down)"
                    );
                }
                Decision::Fire => {
                    match self.orchestrator.enqueue_workflow(workflow_id, "schedule").await {
                        Ok(run_id) => tracing::info!(
                            workflow = %row.get::<String, _>("name"),
                            %run_id,
                            "scheduled run queued"
                        ),
                        Err(e) => {
                            tracing::error!(%workflow_id, error=%e, "failed to queue scheduled run");
                            continue; // leave the bookmark so we retry
                        }
                    }
                }
            }
            sqlx::query("UPDATE workflows SET last_fired_at = $1 WHERE id = $2")
                .bind(now)
                .bind(workflow_id)
                .execute(&self.db.pool)
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cron(expr: &str) -> Cron {
        Cron::from_str(expr).expect("valid cron")
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn waits_until_the_next_occurrence() {
        // Daily at 03:00; last fired at 03:00, now is 14:00 the same day.
        let d = decide(
            &cron("0 3 * * *"),
            Some(at("2026-07-28T03:00:00Z")),
            at("2026-07-28T14:00:00Z"),
            CatchUp::Skip,
            GRACE,
        );
        assert_eq!(d, Decision::Wait);
    }

    #[test]
    fn fires_when_due() {
        // Hourly; last fired 10:00, now 11:00:10 — just past the mark.
        let d = decide(
            &cron("0 * * * *"),
            Some(at("2026-07-28T10:00:00Z")),
            at("2026-07-28T11:00:10Z"),
            CatchUp::Skip,
            GRACE,
        );
        assert_eq!(d, Decision::Fire);
    }

    #[test]
    fn skips_a_long_missed_window_by_default() {
        // Server was down for a day; don't run 24 catch-up jobs.
        let d = decide(
            &cron("0 * * * *"),
            Some(at("2026-07-27T10:00:00Z")),
            at("2026-07-28T14:00:00Z"),
            CatchUp::Skip,
            GRACE,
        );
        assert_eq!(d, Decision::SkipMissed);
    }

    #[test]
    fn run_once_catches_up_on_a_missed_window() {
        let d = decide(
            &cron("0 * * * *"),
            Some(at("2026-07-27T10:00:00Z")),
            at("2026-07-28T14:00:00Z"),
            CatchUp::RunOnce,
            GRACE,
        );
        assert_eq!(d, Decision::Fire);
    }

    #[test]
    fn a_never_fired_workflow_bookmarks_instead_of_firing() {
        // Saving a workflow must not trigger it retroactively — but it must
        // get a bookmark, or it would never become eligible at all.
        let d = decide(
            &cron("0 3 * * *"),
            None,
            at("2026-07-28T14:00:00Z"),
            CatchUp::Skip,
            GRACE,
        );
        assert_eq!(d, Decision::Bookmark);
    }

    #[test]
    fn a_fire_inside_the_grace_window_still_runs() {
        let d = decide(
            &cron("0 * * * *"),
            Some(at("2026-07-28T10:00:00Z")),
            at("2026-07-28T11:04:00Z"), // 4 min late, under the 5 min grace
            CatchUp::Skip,
            GRACE,
        );
        assert_eq!(d, Decision::Fire);
    }

    #[test]
    fn standard_five_field_expressions_parse_as_posix_cron() {
        // Guards against a cron crate that expects a seconds field: 3am
        // daily must mean 03:00, not 00:00:03.
        let next = cron("0 3 * * *")
            .find_next_occurrence(&at("2026-07-28T00:00:00Z"), false)
            .unwrap();
        assert_eq!(next, at("2026-07-28T03:00:00Z"));
    }
}
