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
