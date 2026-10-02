//! Landing: what happens when a card's work reaches done.
//!
//! A card blocked by another waits until that card's work has *landed* — done,
//! not review (see migration 0060). Until now nothing noticed the moment it
//! did: the blocked card sat in the backlog until a person happened to look,
//! which for a chain of cards left running overnight meant the chain stopped
//! at its first link.
//!
//! Six things write `done` — a merge, a drag, an in-place run, an app build, a
//! pull request merged on GitHub, an epic's mirror — and they do not share a
//! code path. So the seam is a column, not a function: `tasks.landed_at` is set
//! once, by whichever of them gets there first ([`land`] only sets it where it
//! is NULL), and the cards it unblocks hear about it once. The writers that can
//! call in straight away do; [`Orchestrator::settle_landings`] sweeps for the
//! rest every scheduler tick, so a writer that forgets costs thirty seconds,
//! not the feature.
//!
//! What a dependent hears is its own choice. `start_when_unblocked` starts it
//! through the same door as the Start button — blocker check, capability vet,
//! row lock and all. Otherwise its thread says it can start, and the attention
//! hook says so to a person who is away.

use sqlx::Row;
use uuid::Uuid;

use crate::db::Db;
use crate::runs::orchestrator::Orchestrator;
use crate::runs::report;

/// A card that the landing left with no blocker standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unblocked {
    pub task_id: Uuid,
    pub title: String,
    pub project_id: Uuid,
    pub project_name: String,
    /// Its owner asked for it to start by itself.
    pub start: bool,
}

/// Record that a card landed, and return the cards that now have nothing in
/// their way.
///
/// Idempotent: a card already marked, or not in done at all, unblocks nothing
/// — so every writer of done can call this without coordinating. Only backlog
/// cards count: one already running, in review or done is past waiting.
pub async fn land(db: &Db, task_id: Uuid) -> anyhow::Result<Option<(String, Vec<Unblocked>)>> {
    let mut tx = db.pool.begin().await?;
    let title: Option<String> = sqlx::query_scalar(
        "UPDATE tasks SET landed_at = now()
          WHERE id = $1 AND board_column = 'done' AND landed_at IS NULL
          RETURNING title",
    )
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(title) = title else {
        return Ok(None);
    };
    let rows = sqlx::query(
        "SELECT t.id, t.title, t.project_id, p.name AS project_name, t.start_when_unblocked
           FROM task_deps d
           JOIN tasks t ON t.id = d.task_id
           JOIN projects p ON p.id = t.project_id
          WHERE d.blocked_by = $1
            AND t.board_column = 'backlog'
            AND NOT EXISTS (
                SELECT 1 FROM task_deps o JOIN tasks b ON b.id = o.blocked_by
                 WHERE o.task_id = t.id AND b.board_column <> 'done')
          ORDER BY t.position",
    )
    .bind(task_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some((
        title,
        rows.iter()
            .map(|r| Unblocked {
                task_id: r.get("id"),
                title: r.get("title"),
                project_id: r.get("project_id"),
                project_name: r.get("project_name"),
                start: r.get("start_when_unblocked"),
            })
            .collect(),
    )))
}

/// What a dependent's thread says, by what became of it. Pure, for the test.
pub fn note(landed: &str, outcome: Result<(), &str>, asked_to_start: bool) -> String {
    match (asked_to_start, outcome) {
        (true, Ok(())) => format!("\u{201c}{landed}\u{201d} landed, so this card started by itself."),
        (true, Err(why)) => format!(
            "\u{201c}{landed}\u{201d} landed, but this card could not start by itself: {why}"
        ),
        (false, _) => format!(
            "\u{201c}{landed}\u{201d} landed — nothing blocks this card now. Start it when you're ready."
        ),
    }
}

/// Why a dependency was not recorded.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BlockerRefusal {
    #[error("a card can't block itself")]
    ItsOwnBlocker,
    #[error("both cards must be on the same board")]
    OtherBoard,
    #[error("that would make these cards wait for each other — neither could ever start")]
    Cycle,
}

/// Record that `task_id` cannot start until `blocked_by` lands.
///
/// Everything that can be wrong is refused now, not at start time: the two
/// cards must share a board, a card cannot block itself, and the edge must
/// not close a cycle — two cards each waiting for the other would simply
/// never run, with nothing anywhere saying why. One place, because a person
/// and an agent (`report_blocker`) both declare dependencies.
pub async fn add_blocker(db: &Db, task_id: Uuid, blocked_by: Uuid) -> anyhow::Result<()> {
    if task_id == blocked_by {
        return Err(BlockerRefusal::ItsOwnBlocker.into());
    }
    let same_project: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM tasks a JOIN tasks b ON a.project_id = b.project_id
          WHERE a.id = $1 AND b.id = $2)",
    )
    .bind(task_id)
    .bind(blocked_by)
    .fetch_one(&db.pool)
    .await?;
    if !same_project {
        return Err(BlockerRefusal::OtherBoard.into());
    }
    // Would this edge close a loop? Walk the new blocker's own blockers all
    // the way up; finding this card there means A→B→…→A.
    let cycles: bool = sqlx::query_scalar(
        "WITH RECURSIVE up AS (
             SELECT blocked_by FROM task_deps WHERE task_id = $2
             UNION
             SELECT d.blocked_by FROM task_deps d JOIN up ON d.task_id = up.blocked_by
         )
         SELECT EXISTS (SELECT 1 FROM up WHERE blocked_by = $1)",
    )
    .bind(task_id)
    .bind(blocked_by)
    .fetch_one(&db.pool)
    .await?;
    if cycles {
        return Err(BlockerRefusal::Cycle.into());
    }
    sqlx::query(
        "INSERT INTO task_deps (task_id, blocked_by) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(task_id)
    .bind(blocked_by)
    .execute(&db.pool)
    .await?;
    Ok(())
}

impl Orchestrator {
    /// A card reached done: mark it landed and wake what it was blocking.
    ///
    /// Best-effort throughout, like every other after-the-fact note: the card
    /// is done whatever happens here, and the sweep will not retry a landing
    /// it has marked, so each failure is logged rather than returned.
    pub async fn landed(&self, task_id: Uuid) {
        let (title, unblocked) = match land(&self.db, task_id).await {
            Ok(Some(found)) => found,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(%task_id, error = %e, "could not record a card landing");
                return;
            }
        };
        for card in unblocked {
            // Started only after `land` committed: `enqueue_task` re-checks
            // the blockers on its own connection, and must see this one done.
            let outcome = if card.start {
                self.start_card(card.task_id).await.map(|_| ())
            } else {
                Ok(())
            };
            let error = outcome.as_ref().err().map(|e| e.to_string());
            let text = note(&title, error.as_deref().map_or(Ok(()), Err), card.start);
            if let Err(e) = report::post_system(&self.db, card.task_id, None, &text).await {
                tracing::warn!(task_id = %card.task_id, error = %e, "could not note an unblocked card");
            }
            // A card that started is visible on the board; the one that is
            // waiting on a person is the news.
            if !card.start || error.is_some() {
                let url = crate::attention::dashboard_url().map(|base| {
                    crate::attention::link(base, Some(card.project_id), Some(card.task_id))
                });
                crate::attention::fire(
                    &self.db,
                    crate::attention::Event::Unblocked,
                    crate::attention::Ctx {
                        title: format!("aichip: \u{201c}{}\u{201d} can start", card.title),
                        body: text,
                        project: Some(card.project_name.clone()),
                        card: Some(card.title.clone()),
                        url,
                        ..Default::default()
                    },
                )
                .await;
            }
        }
    }

    /// Catch every landing no writer reported, and forget the ones that were
    /// taken back. Called each scheduler tick.
    pub async fn settle_landings(&self) -> anyhow::Result<()> {
        // A card dragged out of done has not landed any more; when it comes
        // back, that is a new landing and its dependents hear about it again.
        sqlx::query(
            "UPDATE tasks SET landed_at = NULL
              WHERE landed_at IS NOT NULL AND board_column <> 'done'",
        )
        .execute(&self.db.pool)
        .await?;
        let pending: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM tasks WHERE board_column = 'done' AND landed_at IS NULL LIMIT 100",
        )
        .fetch_all(&self.db.pool)
        .await?;
        for task_id in pending {
            self.landed(task_id).await;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_waiting_on_a_person_is_told_it_can_start() {
        let n = note("Add the schema", Ok(()), false);
        assert!(n.contains("Add the schema") && n.contains("Start it when you're ready"));
    }

    #[test]
    fn a_card_that_started_itself_says_why() {
        assert!(note("Add the schema", Ok(()), true).contains("started by itself"));
    }

    #[test]
    fn a_card_that_could_not_start_says_what_stopped_it() {
        let n = note("Add the schema", Err("the agent is paused"), true);
        assert!(n.contains("could not start") && n.ends_with("the agent is paused"));
    }
}

/// Against a real database and the mock engine — see `crate::testdb`.
#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    /// The loop this module closes, end to end: a card runs, says what it
    /// did on its thread, lands, and the cards it blocked hear about it —
    /// the flagged one by starting, the other by a note.
    #[tokio::test]
    async fn a_landing_starts_the_flagged_dependent_and_tells_the_other() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        // In place: the run settles straight to done, which is a landing.
        let (_, project) = t.project(dir.path(), true).await;
        let a = t.card(project, "schema").await;
        let b = t.card(project, "api").await;
        let c = t.card(project, "docs").await;
        sqlx::query("UPDATE tasks SET start_when_unblocked = TRUE WHERE id = $1")
            .bind(b)
            .execute(&t.db.pool)
            .await
            .unwrap();
        add_blocker(&t.db, b, a).await.unwrap();
        add_blocker(&t.db, c, a).await.unwrap();

        // Blocked cards refuse to start, through the same door as everything.
        assert!(orchestrator.start_card(b).await.is_err());

        orchestrator.start_card(a).await.unwrap();
        t.until(
            "the first card to land",
            "SELECT landed_at IS NOT NULL FROM tasks WHERE id = $1",
            a,
        )
        .await;

        let report: String = sqlx::query_scalar(
            "SELECT content FROM task_comments WHERE task_id = $1 AND author = 'agent' AND run_id IS NOT NULL",
        )
        .bind(a)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        assert!(report.starts_with("**Work report**"), "{report}");

        t.until(
            "the flagged dependent to start",
            "SELECT EXISTS (SELECT 1 FROM runs WHERE task_id = $1)",
            b,
        )
        .await;
        let started: Vec<String> = sqlx::query_scalar(
            "SELECT content FROM task_comments WHERE task_id = $1 AND author = 'system'",
        )
        .bind(b)
        .fetch_all(&t.db.pool)
        .await
        .unwrap();
        assert_eq!(started, vec![note("schema", Ok(()), true)]);

        let told: Vec<String> = sqlx::query_scalar(
            "SELECT content FROM task_comments WHERE task_id = $1 AND author = 'system'",
        )
        .bind(c)
        .fetch_all(&t.db.pool)
        .await
        .unwrap();
        assert_eq!(told, vec![note("schema", Ok(()), false)]);
        let c_runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE task_id = $1")
            .bind(c)
            .fetch_one(&t.db.pool)
            .await
            .unwrap();
        assert_eq!(c_runs, 0, "an unflagged dependent waits for a person");

        t.finish().await;
    }

    /// However many writers of done notice one landing, its dependents hear
    /// about it once; taken back out of done, it is news again.
    #[tokio::test]
    async fn a_landing_is_news_once_until_it_is_taken_back() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        let (_, project) = t.project(dir.path(), false).await;
        let a = t.card(project, "a").await;
        let b = t.card(project, "b").await;
        add_blocker(&t.db, b, a).await.unwrap();

        // Not done: nothing has landed.
        assert!(land(&t.db, a).await.unwrap().is_none());

        let done = |id| {
            sqlx::query("UPDATE tasks SET board_column = $2 WHERE id = $1")
                .bind(id)
                .bind("done")
        };
        done(a).execute(&t.db.pool).await.unwrap();
        let (title, unblocked) = land(&t.db, a).await.unwrap().unwrap();
        assert_eq!(title, "a");
        assert_eq!(
            unblocked.iter().map(|u| u.task_id).collect::<Vec<_>>(),
            vec![b]
        );
        assert!(
            land(&t.db, a).await.unwrap().is_none(),
            "the second notice is silent"
        );

        sqlx::query("UPDATE tasks SET board_column = 'review' WHERE id = $1")
            .bind(a)
            .execute(&t.db.pool)
            .await
            .unwrap();
        orchestrator.settle_landings().await.unwrap();
        done(a).execute(&t.db.pool).await.unwrap();
        assert!(
            land(&t.db, a).await.unwrap().is_some(),
            "landing again is news again"
        );

        t.finish().await;
    }

    #[tokio::test]
    async fn a_dependency_that_would_close_a_loop_is_refused() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (_, project) = t.project(dir.path(), false).await;
        let (_, elsewhere) = t.project(&dir.path().join("other"), false).await;
        let a = t.card(project, "a").await;
        let b = t.card(project, "b").await;
        let c = t.card(project, "c").await;
        let far = t.card(elsewhere, "far").await;
        add_blocker(&t.db, b, a).await.unwrap();
        add_blocker(&t.db, c, b).await.unwrap();
        let refusal = |e: anyhow::Error| e.downcast::<BlockerRefusal>().unwrap();
        assert_eq!(
            refusal(add_blocker(&t.db, a, c).await.unwrap_err()),
            BlockerRefusal::Cycle
        );
        assert_eq!(
            refusal(add_blocker(&t.db, a, a).await.unwrap_err()),
            BlockerRefusal::ItsOwnBlocker
        );
        assert_eq!(
            refusal(add_blocker(&t.db, a, far).await.unwrap_err()),
            BlockerRefusal::OtherBoard
        );
        t.finish().await;
    }
}
