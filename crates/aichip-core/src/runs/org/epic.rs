//! Turning a manager's plan into board tickets.
//!
//! When a team is handed a card, the manager breaks the goal into assignments —
//! each briefed, owned, sized and ordered. Those live as `steps`, which is the
//! right home for *execution*: they carry dependencies, file scopes, attempts
//! and session ids. But a step is not something a person can work with. It is
//! invisible on the board, it cannot be commented on or reassigned, it cannot be
//! run on its own, and it is deleted with its run.
//!
//! So each assignment also gets a real card, hanging under the original as its
//! epic. The two representations divide cleanly:
//!
//! * **The step is the unit of execution.** `work_phase` reads assignees,
//!   dependencies and scopes from `steps`, never from a card.
//! * **The card is the unit of tracking.** It outlives the run and is the thing
//!   a person moves, comments on, and re-runs.
//!
//! Which leaves the question of who may write what, and the answer is
//! *exclusive and time-sliced rather than shared*: while a step is live the org
//! owns its card and the routes refuse to let a person move or delete it; once
//! the step is terminal the mirror writes the column one last time and never
//! touches it again. Title, prompt and assignee are written **once**, at
//! creation, so an edit made by hand is never silently reverted.

use crate::db::Db;
use crate::tasks::{create_child, resolve_agent_by_name, NewChildTask};
use sqlx::Row;
use uuid::Uuid;

/// Which step keys are work assigned out rather than the manager thinking.
/// `org::mod` is the authority on this; it is imported rather than restated so
/// the two cannot drift.
use super::NOT_AN_ASSIGNMENT;

/// Give every assignment in this run a card, and point each card's column at
/// its step. Idempotent, and a no-op for runs that came from no card.
pub(crate) async fn reconcile(db: &Db, run_id: Uuid) -> anyhow::Result<()> {
    create_missing(db, run_id).await?;
    mirror_run(db, run_id).await
}

/// Mirror one step onto its card.
///
/// Targeted rather than a full pass, because this runs on every status
/// transition while the batch loop may hold for many minutes.
pub(crate) async fn mirror_step(db: &Db, step_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(&format!(
        "UPDATE tasks t SET board_column = {COLUMN_FOR_STEP}
         FROM steps s, projects p
         WHERE s.task_id = t.id AND p.id = t.project_id
           AND s.id = $1 {NOT_OWNED_BY_ITS_OWN_RUN}"
    ))
    .bind(step_id)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Mirror every linked step in a run, for the end of a run and for boot
/// recovery. Cheap on runs with no linked steps, which is all of them but org
/// runs started from a card.
pub(crate) async fn mirror_run(db: &Db, run_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(&format!(
        "UPDATE tasks t SET board_column = {COLUMN_FOR_STEP}
         FROM steps s, projects p
         WHERE s.task_id = t.id AND p.id = t.project_id
           AND s.run_id = $1 {NOT_OWNED_BY_ITS_OWN_RUN}"
    ))
    .bind(run_id)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Tell an assignment's card that the manager rewrote its brief.
///
/// The counterpart to never re-syncing content. A re-plan can change a queued
/// assignment's brief or owner, and the card has to hear about it — but as a
/// comment, so that whatever a person has typed into the card survives.
pub(crate) async fn note_revision(
    db: &Db,
    step_id: Uuid,
    author: &str,
    note: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO task_comments (task_id, author, content)
         SELECT s.task_id, 'agent', $2 FROM steps s
          WHERE s.id = $1 AND s.task_id IS NOT NULL",
    )
    .bind(step_id)
    .bind(format!("**{author}** {note}"))
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Where a card sits, given the state of its assignment.
///
/// Four columns cannot express nine step states, and adding a fifth would mean
/// touching every literal comparison in the server, the board, the search index
/// and the TypeScript union — to grow every user's board for this. So the column
/// carries *position* and the card's badge carries the outcome: `failed` and
/// `canceled` both land in Review, which already means "a human needs to look at
/// this", and the badge says which it was.
///
/// The `review` / `done` split follows `projects.vcs` the way
/// `settle_task_for_run` does. An in-place project has no diff, so parking a
/// finished card in Review would offer a review that cannot happen.
///
/// `skipped` is a dropped assignment — nothing was done, so `done` is not quite
/// true, but leaving it in Backlog is worse: it would look like queued work that
/// will eventually run.
const COLUMN_FOR_STEP: &str = "CASE
    WHEN s.status = 'queued' THEN 'backlog'
    WHEN s.status IN ('starting','running','waiting_permission','rate_limited') THEN 'running'
    WHEN s.status = 'skipped' THEN 'done'
    WHEN s.status = 'completed' AND p.vcs = 'git' THEN 'review'
    WHEN s.status = 'completed' THEN 'done'
    ELSE 'review'
  END";

/// Stop mirroring a card once it has a run of its own.
///
/// Pressing Run on a sub-ticket starts an ordinary single-agent run, and that
/// run's own `settle_task_for_run` owns the column from then on. Without this
/// the two would fight: the org's stale step status would keep overwriting the
/// column the card's own run had just set.
const NOT_OWNED_BY_ITS_OWN_RUN: &str =
    "AND NOT EXISTS (SELECT 1 FROM runs own WHERE own.task_id = t.id AND own.team_id IS NULL)";

/// Create a card for every assignment that has never had one.
async fn create_missing(db: &Db, run_id: Uuid) -> anyhow::Result<()> {
    let run = sqlx::query(
        "SELECT r.task_id, r.project_id, r.engine, t.worktree_path, t.branch
           FROM runs r JOIN tasks t ON t.id = r.task_id
          WHERE r.id = $1",
    )
    .bind(run_id)
    .fetch_optional(&db.pool)
    .await?;
    // No originating card means nothing to hang tickets under. An org run
    // launched straight from the team room is a legitimate case, not an error,
    // and parentless "children" would be surprising.
    let Some(run) = run else { return Ok(()) };

    let parent_id: Uuid = run.get("task_id");
    let project_id: Uuid = run.get("project_id");
    let engine: String = run.get("engine");
    let worktree_path: Option<String> = run.get("worktree_path");
    let branch: Option<String> = run.get("branch");

    let pending = sqlx::query(&format!(
        "SELECT id, title, brief, assignee, done_when, position
           FROM steps
          WHERE run_id = $1 AND task_linked_at IS NULL AND {NOT_AN_ASSIGNMENT}
          ORDER BY position, step_key"
    ))
    .bind(run_id)
    .fetch_all(&db.pool)
    .await?;

    for (index, step) in pending.iter().enumerate() {
        let step_id: Uuid = step.get("id");
        let title: Option<String> = step.get("title");
        let brief: Option<String> = step.get("brief");
        let assignee: Option<String> = step.get("assignee");
        let done_when: Vec<String> = step.get("done_when");
        let position: Option<f64> = step.get("position");

        // Claim the step first, so two concurrent reconciles cannot both decide
        // this assignment needs a card.
        let claimed = sqlx::query(
            "UPDATE steps SET task_linked_at = now()
              WHERE id = $1 AND task_linked_at IS NULL",
        )
        .bind(step_id)
        .execute(&db.pool)
        .await?;
        if claimed.rows_affected() == 0 {
            continue;
        }

        let agent_id = match assignee.as_deref() {
            Some(name) => resolve_agent_by_name(db, project_id, name).await?,
            None => None,
        };
        let task_id = create_child(
            db,
            NewChildTask {
                parent_id,
                project_id,
                title: title.unwrap_or_else(|| "Untitled assignment".to_string()),
                prompt: brief_as_prompt(brief.as_deref().unwrap_or(""), &done_when),
                agent_id,
                engine: engine.clone(),
                worktree_path: worktree_path.clone(),
                branch: branch.clone(),
                order_hint: position.unwrap_or(index as f64 + 1.0),
            },
        )
        .await?;

        sqlx::query("UPDATE steps SET task_id = $2 WHERE id = $1")
            .bind(step_id)
            .bind(task_id)
            .execute(&db.pool)
            .await?;
    }
    Ok(())
}

/// The brief, plus the acceptance criteria the manager set.
///
/// The criteria are part of the prompt rather than decoration: a card whose
/// prompt is a bare brief is not independently runnable, and being able to run
/// one on its own is the whole reason it is a card.
fn brief_as_prompt(brief: &str, done_when: &[String]) -> String {
    let mut prompt = brief.trim().to_string();
    if !done_when.is_empty() {
        prompt.push_str("\n\nDone when:\n");
        for item in done_when {
            prompt.push_str("- ");
            prompt.push_str(item.trim());
            prompt.push('\n');
        }
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_when_becomes_acceptance_criteria() {
        let prompt = brief_as_prompt(
            "  Parse the upload.  ",
            &["headers are validated".into(), "bad rows report a line".into()],
        );
        assert_eq!(
            prompt,
            "Parse the upload.\n\nDone when:\n- headers are validated\n- bad rows report a line\n"
        );
    }

    #[test]
    fn a_brief_with_no_criteria_is_left_alone() {
        // No trailing "Done when:" heading with nothing under it.
        assert_eq!(brief_as_prompt("Just do it.", &[]), "Just do it.");
    }

    /// The mapping is SQL, so this asserts the shape a reader relies on rather
    /// than executing it: every state the executor can leave a step in has a
    /// column, and none of them is Backlog once work has begun.
    #[test]
    fn every_step_state_is_mapped() {
        for state in [
            "queued",
            "starting",
            "running",
            "waiting_permission",
            "rate_limited",
            "completed",
            "failed",
            "canceled",
            "skipped",
        ] {
            let quoted = format!("'{state}'");
            assert!(
                COLUMN_FOR_STEP.contains(&quoted) || state == "failed" || state == "canceled",
                "{state} has no branch, and would silently fall through to Review"
            );
        }
        // Failure and cancellation are the deliberate fall-through.
        assert!(COLUMN_FOR_STEP.contains("ELSE 'review'"));
    }
}
