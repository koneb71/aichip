//! Follow-up runs: a scoped agent pass in a card's existing worktree.
//!
//! The reviewable unit in aichip is a card's worktree and the diff it makes.
//! A follow-up is a run that goes back into that worktree to act on something
//! said *about* the diff — so the result lands on the same branch, in the same
//! diff the person was reading, instead of a fresh attempt that starts over.
//!
//! It used to exist in one form only, a review note, and that form failed at
//! dispatch: it carried `comment_id`, which every other reader takes to mean
//! "a comment reply", so the dispatcher sent it down the reply path, whose
//! query requires an agent a fix run never has. It also showed in the thread
//! as "an agent is replying…", was billed as a mention, and could never be
//! resumed. A follow-up now records its note in `review_comment_id`, and
//! nothing reads that as anything else.

use crate::runs::orchestrator::{clip_chars, AlreadyRunning, Orchestrator};
use sqlx::Row;
use uuid::Uuid;

/// What a follow-up acts on.
#[derive(Debug, Clone)]
pub enum FollowUp {
    /// A person's note on the diff, anchored to a line or to the whole change.
    ReviewNote { comment_id: Uuid },
}

impl FollowUp {
    /// `runs.trigger`, which is also how the history names the run.
    fn trigger(&self) -> &'static str {
        match self {
            Self::ReviewNote { .. } => "review",
        }
    }

    /// Above a normal task run (10): someone is sitting there reading the diff.
    fn priority(&self) -> i32 {
        14
    }
}

/// Why a follow-up could not start. Each is something the person can see and
/// act on, so the routes answer them as conflicts rather than server errors.
#[derive(Debug, thiserror::Error)]
pub enum FollowUpRefusal {
    #[error("this card has no worktree to work in — it was merged, discarded, or never ran")]
    NoWorktree,
    #[error("this card is done; start it again rather than following up on it")]
    Done,
    #[error("that note does not belong to this card")]
    ForeignNote,
}

impl Orchestrator {
    /// Queue a follow-up run in the card's own worktree.
    ///
    /// Serialised with `enqueue_task` on the card's row, and refused while any
    /// run of the card is live: two agents writing one worktree is the thing
    /// worktrees exist to prevent.
    pub async fn enqueue_follow_up(
        &self,
        task_id: Uuid,
        follow_up: FollowUp,
    ) -> anyhow::Result<Uuid> {
        let mut guard = self.db.pool.begin().await?;
        // The bound agent's engine wins over the card's, as it does for every
        // other start of the card — the old review fix used the card's alone,
        // so a card bound to an OpenCode agent was fixed by Claude Code.
        let card = sqlx::query(
            "SELECT t.prompt, t.board_column, t.worktree_path,
                    COALESCE(a.engine, t.engine) AS engine
               FROM tasks t LEFT JOIN agents a ON a.id = t.agent_id
              WHERE t.id = $1
                FOR NO KEY UPDATE OF t",
        )
        .bind(task_id)
        .fetch_one(&mut *guard)
        .await?;

        let running: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM runs WHERE task_id = $1
                               AND status NOT IN ('completed','failed','canceled'))",
        )
        .bind(task_id)
        .fetch_one(&mut *guard)
        .await?;
        if running {
            return Err(AlreadyRunning.into());
        }
        if card.get::<String, _>("board_column") == "done" {
            return Err(FollowUpRefusal::Done.into());
        }
        let worktree: Option<String> = card.get("worktree_path");
        if !worktree.is_some_and(|w| std::path::Path::new(&w).is_dir()) {
            return Err(FollowUpRefusal::NoWorktree.into());
        }

        let task_prompt: String = card.get("prompt");
        let (prompt, review_comment_id) = match &follow_up {
            FollowUp::ReviewNote { comment_id } => {
                let note = sqlx::query(
                    "SELECT task_id, content, file_path, line, hunk
                       FROM task_comments WHERE id = $1",
                )
                .bind(comment_id)
                .fetch_one(&mut *guard)
                .await?;
                if note.get::<Uuid, _>("task_id") != task_id {
                    return Err(FollowUpRefusal::ForeignNote.into());
                }
                let prompt = review_fix_prompt(
                    &task_prompt,
                    note.get::<Option<String>, _>("file_path").as_deref(),
                    note.get::<Option<i32>, _>("line"),
                    note.get::<Option<String>, _>("hunk").as_deref(),
                    &note.get::<String, _>("content"),
                );
                (prompt, Some(*comment_id))
            }
        };

        let run_id: Uuid = sqlx::query_scalar(
            "INSERT INTO runs (task_id, review_comment_id, prompt_override, status, trigger, engine)
             VALUES ($1, $2, $3, 'queued', $4, $5) RETURNING id",
        )
        .bind(task_id)
        .bind(review_comment_id)
        .bind(&prompt)
        .bind(follow_up.trigger())
        .bind(card.get::<String, _>("engine"))
        .fetch_one(&mut *guard)
        .await?;
        sqlx::query("INSERT INTO queue (run_id, priority) VALUES ($1, $2)")
            .bind(run_id)
            .bind(follow_up.priority())
            .execute(&mut *guard)
            .await?;
        guard.commit().await?;
        Ok(run_id)
    }
}

/// Turn a review note into a brief for the agent.
///
/// Pure so the shape can be tested without a database. Three things have to
/// survive into the prompt: where the note points, what the code looked like
/// when it was written, and a scope limit — a review note is not licence to
/// keep working on the task.
pub(crate) fn review_fix_prompt(
    task_prompt: &str,
    file_path: Option<&str>,
    line: Option<i32>,
    hunk: Option<&str>,
    note: &str,
) -> String {
    let mut prompt =
        String::from("You are acting on review feedback for work you already did.\n\n");
    match (file_path, line) {
        (Some(path), Some(line)) => prompt.push_str(&format!(
            "The reviewer commented on {path}, around line {line}.\n"
        )),
        (Some(path), None) => prompt.push_str(&format!("The reviewer commented on {path}.\n")),
        _ => prompt.push_str("The reviewer commented on the change as a whole.\n"),
    }
    if let Some(hunk) = hunk.filter(|h| !h.trim().is_empty()) {
        // Line numbers drift the moment you edit; the snapshot is what
        // actually identifies the code being talked about.
        prompt.push_str(&format!(
            "\nThe code as it stood when they wrote the note:\n```diff\n{}\n```\n",
            clip_chars(hunk, 2000),
        ));
    }
    prompt.push_str(&format!("\nTheir note:\n{note}\n"));
    prompt.push_str(&format!(
        "\nFor context, the original task was:\n{}\n",
        clip_chars(task_prompt, 800),
    ));
    prompt.push_str(
        "\nMake exactly this change and stop. Do not refactor beyond it, do not \
         revisit other review notes, and do not continue the original task. If \
         the note is a question rather than a request, answer it without editing \
         anything. Finish with one short line saying what you changed.",
    );
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_review_note_becomes_a_scoped_brief() {
        let prompt = review_fix_prompt(
            "Build the leads finder",
            Some("backend/app/routes.py"),
            Some(42),
            Some("- return None\n+ return leads"),
            "This swallows the error; raise instead.",
        );
        assert!(prompt.contains("backend/app/routes.py"));
        assert!(prompt.contains("line 42"));
        assert!(prompt.contains("return leads"), "the hunk grounds the note");
        assert!(prompt.contains("raise instead"));
        // The scope limit is the point: a review note must not restart the task.
        assert!(prompt.contains("do not continue the original task"));
    }

    #[test]
    fn a_note_without_an_anchor_still_works() {
        // Card-level review feedback has no file or line.
        let prompt = review_fix_prompt("Do the thing", None, None, None, "Rename the module.");
        assert!(prompt.contains("the change as a whole"));
        assert!(prompt.contains("Rename the module."));
        assert!(!prompt.contains("```diff"), "no hunk, no empty code fence");
    }

    #[test]
    fn a_huge_hunk_cannot_crowd_out_the_note() {
        let prompt = review_fix_prompt(
            &"task ".repeat(1000),
            Some("a.rs"),
            Some(1),
            Some(&"x".repeat(50_000)),
            "Fix it.",
        );
        assert!(prompt.chars().count() < 4000);
        assert!(prompt.contains("Fix it."));
    }
}
