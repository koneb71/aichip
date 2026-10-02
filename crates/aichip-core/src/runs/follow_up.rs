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

use crate::checks::CheckResult;
use crate::runs::orchestrator::{clip_chars, AlreadyRunning, Orchestrator};
use sqlx::Row;
use uuid::Uuid;

/// What a follow-up acts on.
#[derive(Debug, Clone)]
pub enum FollowUp {
    /// A person's note on the diff, anchored to a line or to the whole change.
    ReviewNote { comment_id: Uuid },
    /// A check run on the card's worktree that did not pass.
    FailingChecks { check_run_id: Uuid },
    /// The base branch, brought into the card's branch, conflicted here.
    MergeConflict { files: Vec<String>, base: String },
    /// A completed run that said nothing. One read-only pass, continuing its
    /// session where the engine can, writes the card's report.
    Summarize { run_id: Uuid },
    /// A person answered the question the card's agent asked. The asking
    /// run's session continues, in the same worktree, with the answer.
    Answer { question_id: Uuid },
}

impl FollowUp {
    /// `runs.trigger`, which is also how the history names the run.
    fn trigger(&self) -> &'static str {
        match self {
            Self::ReviewNote { .. } => "review",
            Self::FailingChecks { .. } => "checks",
            Self::MergeConflict { .. } => "conflict",
            Self::Summarize { .. } => "summary",
            Self::Answer { .. } => "answer",
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
    #[error("those checks do not belong to this card")]
    ForeignChecks,
    #[error("those checks did not fail, so there is nothing to fix")]
    NothingFailed,
    #[error("that run does not belong to this card")]
    ForeignRun,
    #[error("that question does not belong to this card, or has not been answered")]
    ForeignQuestion,
}

impl Orchestrator {
    /// Anything a person does to a card outranks aichip asking a silent run
    /// to explain itself. A summary pass is a live run like any other, and
    /// left alone it made Merge, Update from main, a review fix or a
    /// reassignment refuse with "an agent is still working on this card" —
    /// for hours, when the queue was holding. So it gives way: the doors
    /// that would refuse call this first, and the summary is dropped.
    pub async fn supersede_summary(&self, task_id: Uuid) -> anyhow::Result<()> {
        let live = || {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM runs WHERE task_id = $1 AND trigger = 'summary'
                    AND status NOT IN ('completed', 'failed', 'canceled')",
            )
            .bind(task_id)
            .fetch_all(&self.db.pool)
        };
        let pending = live().await?;
        if pending.is_empty() {
            return Ok(());
        }
        for run_id in &pending {
            if !self.cancel(*run_id) {
                self.cancel_idle(*run_id).await?;
            }
        }
        // One that was executing ends a moment later, through `finish`. Wait
        // for it, so the caller's own "is anything running" check sees it
        // gone rather than refusing on the run it just stopped.
        for _ in 0..50 {
            if live().await?.is_empty() {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Ok(())
    }

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
        if !matches!(follow_up, FollowUp::Summarize { .. }) {
            self.supersede_summary(task_id).await?;
        }
        let mut guard = self.db.pool.begin().await?;
        // The bound agent's engine wins over the card's, as it does for every
        // other start of the card — the old review fix used the card's alone,
        // so a card bound to an OpenCode agent was fixed by Claude Code.
        let card = sqlx::query(
            "SELECT t.prompt, t.board_column, t.worktree_path, p.default_branch,
                    t.agent_id, COALESCE(a.engine, t.engine) AS engine
               FROM tasks t
               JOIN projects p ON p.id = t.project_id
               LEFT JOIN agents a ON a.id = t.agent_id
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
        let agent: Option<Uuid> = card.get("agent_id");
        crate::agents::assert_can_run(&self.db, agent.as_slice()).await?;
        crate::budgets::check(
            &self.db,
            &crate::budgets::scope_of_task(&self.db, task_id).await?,
            true,
        )
        .await?;
        let worktree: Option<String> = card.get("worktree_path");
        if !worktree
            .as_deref()
            .is_some_and(|w| std::path::Path::new(w).is_dir())
        {
            return Err(FollowUpRefusal::NoWorktree.into());
        }

        let task_prompt: String = card.get("prompt");
        let engine_id: String = card.get("engine");
        let mut session: Option<(String, String)> = None;
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
            FollowUp::FailingChecks { check_run_id } => {
                let checks =
                    sqlx::query("SELECT task_id, status, results FROM check_runs WHERE id = $1")
                        .bind(check_run_id)
                        .fetch_one(&mut *guard)
                        .await?;
                if checks.get::<Uuid, _>("task_id") != task_id {
                    return Err(FollowUpRefusal::ForeignChecks.into());
                }
                let results: Vec<CheckResult> = serde_json::from_value(checks.get("results"))?;
                if checks.get::<String, _>("status") != "failed"
                    || results.iter().all(CheckResult::passed)
                {
                    return Err(FollowUpRefusal::NothingFailed.into());
                }
                (checks_fix_prompt(&task_prompt, &results), None)
            }
            FollowUp::MergeConflict { files, base } => {
                (conflict_prompt(&task_prompt, files, base), None)
            }
            FollowUp::Summarize { run_id } => {
                let prior = sqlx::query(
                    "SELECT task_id, session_id, session_engine FROM runs WHERE id = $1",
                )
                .bind(run_id)
                .fetch_one(&mut *guard)
                .await?;
                if prior.get::<Option<Uuid>, _>("task_id") != Some(task_id) {
                    return Err(FollowUpRefusal::ForeignRun.into());
                }
                // Continuing the run's own session is what makes the summary
                // worth having — it remembers what it did. Only for an engine
                // that can resume, and only a session that engine minted.
                let can_resume = self
                    .engine(&engine_id)
                    .is_some_and(|e| e.capabilities().resume_sessions);
                if let (true, Some(sid), Some(minted_by)) = (
                    can_resume,
                    prior.get::<Option<String>, _>("session_id"),
                    prior.get::<Option<String>, _>("session_engine"),
                ) {
                    if minted_by == engine_id {
                        session = Some((sid, minted_by));
                    }
                }
                // Without the session, the changed files are the evidence.
                let files: Vec<String> = match worktree.as_deref() {
                    Some(dir) => self
                        .worktrees
                        .diff_stat(
                            std::path::Path::new(dir),
                            &card.get::<String, _>("default_branch"),
                        )
                        .await
                        .map(|stats| stats.into_iter().map(|f| f.path).collect())
                        .unwrap_or_default(),
                    None => vec![],
                };
                (
                    summary_prompt(&task_prompt, &files, session.is_some()),
                    None,
                )
            }
            FollowUp::Answer { question_id } => {
                let q = sqlx::query(
                    "SELECT q.task_id, q.question, q.answer, r.session_id, r.session_engine
                       FROM run_questions q JOIN runs r ON r.id = q.run_id
                      WHERE q.id = $1",
                )
                .bind(question_id)
                .fetch_one(&mut *guard)
                .await?;
                let answer: Option<String> = q.get("answer");
                if q.get::<Uuid, _>("task_id") != task_id || answer.is_none() {
                    return Err(FollowUpRefusal::ForeignQuestion.into());
                }
                // The asker remembers why it asked; carrying its session is
                // what makes a one-line answer enough.
                let can_resume = self
                    .engine(&engine_id)
                    .is_some_and(|e| e.capabilities().resume_sessions);
                if let (true, Some(sid), Some(minted_by)) = (
                    can_resume,
                    q.get::<Option<String>, _>("session_id"),
                    q.get::<Option<String>, _>("session_engine"),
                ) {
                    if minted_by == engine_id {
                        session = Some((sid, minted_by));
                    }
                }
                (
                    answer_prompt(
                        &task_prompt,
                        &q.get::<String, _>("question"),
                        answer.as_deref().unwrap_or_default(),
                        session.is_some(),
                    ),
                    None,
                )
            }
        };

        let (session_id, session_engine) = session.unzip();
        let run_id: Uuid = sqlx::query_scalar(
            "INSERT INTO runs (task_id, review_comment_id, prompt_override, status, trigger, engine,
                               session_id, session_engine)
             VALUES ($1, $2, $3, 'queued', $4, $5, $6, $7) RETURNING id",
        )
        .bind(task_id)
        .bind(review_comment_id)
        .bind(&prompt)
        .bind(follow_up.trigger())
        .bind(&engine_id)
        .bind(session_id)
        .bind(session_engine)
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

/// Turn failing checks into a brief for the agent.
///
/// The output is the evidence, so it is kept — but from its end, where a test
/// runner puts the failures and the summary, and bounded per check so one
/// enormous log cannot crowd out the rest. The rules at the bottom are the
/// point: the way to make a red check green that an agent reaches for first is
/// to delete the test, and that is not a fix.
pub(crate) fn checks_fix_prompt(task_prompt: &str, results: &[CheckResult]) -> String {
    let failing: Vec<&CheckResult> = results.iter().filter(|r| !r.passed()).collect();
    let mut prompt = String::from(
        "The checks configured for this project failed on the work you already did. \
         Make them pass.\n",
    );
    for r in failing.iter().take(5) {
        let how = if r.timed_out {
            "it did not finish within its time limit".to_string()
        } else {
            match r.exit_code {
                Some(code) => format!("it exited with status {code}"),
                None => "it could not run".to_string(),
            }
        };
        prompt.push_str(&format!(
            "\n### {}\nCommand: `{}` — {how}.\nThe end of its output:\n```\n{}\n```\n",
            r.name,
            r.command,
            clip_tail(&r.output_tail, 3000),
        ));
    }
    if failing.len() > 5 {
        prompt.push_str(&format!(
            "\n…and {} more failing checks.\n",
            failing.len() - 5
        ));
    }
    let passing: Vec<&str> = results
        .iter()
        .filter(|r| r.passed())
        .map(|r| r.name.as_str())
        .collect();
    if !passing.is_empty() {
        prompt.push_str(&format!(
            "\nThese passed and must keep passing: {}.\n",
            passing.join(", ")
        ));
    }
    prompt.push_str(&format!(
        "\nFor context, the original task was:\n{}\n",
        clip_chars(task_prompt, 800),
    ));
    prompt.push_str(
        "\nFix the code, not the checks: do not delete, skip or loosen tests, and do not \
         change how the checks are run. If a failure has nothing to do with your change, \
         say so in one line and stop without editing. Finish with one short line saying \
         what you changed.",
    );
    prompt
}

/// Brief an agent to resolve a merge conflict left in its worktree.
///
/// The markers in the files are the real brief; this says where they are and
/// fences off the git commands that would undo or bypass the merge. aichip
/// concludes the merge itself once no marker is left, and refuses to while
/// one is — so the agent's whole job is the edit.
pub(crate) fn conflict_prompt(task_prompt: &str, files: &[String], base: &str) -> String {
    let listed: Vec<String> = files.iter().take(30).map(|f| format!("- {f}")).collect();
    let mut prompt = format!(
        "{base} has moved on since you started, and bringing it into this branch \
         conflicted. The merge is in progress in your working directory, with \
         conflict markers in:\n{}\n",
        listed.join("\n")
    );
    if files.len() > 30 {
        prompt.push_str(&format!("…and {} more.\n", files.len() - 30));
    }
    prompt.push_str(&format!(
        "\nFor context, your task was:\n{}\n",
        clip_chars(task_prompt, 800),
    ));
    prompt.push_str(
        "\nResolve every conflict so both sides' intent survives: keep what the base \
         branch changed and what your work changed, reconciling them where they \
         overlap. Remove every conflict marker. Edit the files only — do not run \
         git commit, git merge --abort, git reset, git checkout or git stash; the \
         merge is concluded for you when no markers are left. Finish with one line \
         per file saying how you resolved it.",
    );
    prompt
}

/// Ask a run that finished without a word to say what it did.
///
/// Read-only by construction — the run is dispatched with the planning pass's
/// denied tools — and worded so the answer is the report itself, written for
/// the person about to review the diff.
pub(crate) fn summary_prompt(task_prompt: &str, files: &[String], resumed: bool) -> String {
    let mut prompt = String::from(if resumed {
        "You just finished working on this task but did not say what you did. "
    } else {
        "Work was just finished on this task, but no account of it was left. "
    });
    prompt.push_str(
        "Write the report a person reviewing the change needs: what changed and why, \
         anything left unfinished or uncertain, and what to check first. Do not edit \
         anything; answer in a few short paragraphs or a list.\n",
    );
    if !files.is_empty() {
        let listed: Vec<String> = files.iter().take(40).map(|f| format!("- {f}")).collect();
        prompt.push_str(&format!(
            "\nFiles the change touches:\n{}\n",
            listed.join("\n")
        ));
        if files.len() > 40 {
            prompt.push_str(&format!("…and {} more.\n", files.len() - 40));
        }
    }
    prompt.push_str(&format!(
        "\nThe task was:\n{}\n",
        clip_chars(task_prompt, 800)
    ));
    prompt
}

/// A person's answer to the question the agent asked, as the next turn.
///
/// Fenced: the answer is a person's words, but it travels into a prompt next
/// to instructions, and the fence is what keeps "ignore the above" in an
/// answer an answer.
pub(crate) fn answer_prompt(
    task_prompt: &str,
    question: &str,
    answer: &str,
    resumed: bool,
) -> String {
    let mut prompt = String::from(if resumed {
        "You asked the person a question and stopped. They have answered. Carry on with the task from where you left off, using their answer.\n"
    } else {
        "Earlier work on this task stopped to ask the person a question. They have answered. Pick the task up in this worktree — the changes so far are already here — and carry on, using their answer.\n"
    });
    prompt.push_str(&format!(
        "\nThe question:\n{}\n\nTheir answer:\n{}\n",
        clip_chars(question, 1000),
        crate::fence::wrap(
            crate::fence::ANSWER_BEGIN,
            crate::fence::ANSWER_END,
            &clip_chars(answer, 2000)
        ),
    ));
    if !resumed {
        prompt.push_str(&format!(
            "\nThe task was:\n{}\n",
            clip_chars(task_prompt, 1500)
        ));
    }
    prompt
}

/// The last `max` characters, marking that the start was dropped.
fn clip_tail(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    "…\n".to_string() + &s.chars().skip(count - max).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(name: &str, exit: Option<i32>, timed_out: bool, output: &str) -> CheckResult {
        CheckResult {
            name: name.into(),
            command: format!("run {name}"),
            exit_code: exit,
            timed_out,
            ms: 10,
            output_tail: output.into(),
        }
    }

    #[test]
    fn failing_checks_become_a_brief_with_their_evidence() {
        let prompt = checks_fix_prompt(
            "Add CSV export",
            &[
                result("tests", Some(101), false, "test export::csv ... FAILED"),
                result("lint", Some(0), false, "ok"),
                result("e2e", None, true, "waiting…"),
            ],
        );
        assert!(prompt.contains("### tests") && prompt.contains("status 101"));
        assert!(
            prompt.contains("export::csv ... FAILED"),
            "the output is the evidence"
        );
        assert!(prompt.contains("did not finish within its time limit"));
        assert!(prompt.contains("must keep passing: lint"));
        assert!(
            !prompt.contains("### lint"),
            "a passing check is not a failure"
        );
        // The rule that matters most: deleting the test is not a fix.
        assert!(prompt.contains("do not delete, skip or loosen tests"));
    }

    #[test]
    fn a_conflict_brief_names_the_files_and_fences_off_git() {
        let prompt = conflict_prompt(
            "Add CSV export",
            &["src/a.rs".into(), "README.md".into()],
            "main",
        );
        assert!(prompt.contains("main has moved on"));
        assert!(prompt.contains("- src/a.rs") && prompt.contains("- README.md"));
        assert!(prompt.contains("do not run git commit, git merge --abort"));
        assert!(prompt.contains("Add CSV export"));
    }

    #[test]
    fn a_summary_brief_is_read_only_and_names_the_files() {
        let prompt = summary_prompt("Add CSV export", &["src/export.rs".into()], true);
        assert!(prompt.contains("You just finished"));
        assert!(prompt.contains("Do not edit anything"));
        assert!(prompt.contains("- src/export.rs"));
        let fresh = summary_prompt("Add CSV export", &[], false);
        assert!(fresh.contains("no account of it was left"));
        assert!(!fresh.contains("Files the change touches"));
    }

    #[test]
    fn a_huge_log_keeps_its_end() {
        let log = format!("{}\nFINAL: 1 failed", "noise\n".repeat(10_000));
        let prompt = checks_fix_prompt("t", &[result("tests", Some(1), false, &log)]);
        assert!(
            prompt.contains("FINAL: 1 failed"),
            "the summary is at the end"
        );
        assert!(prompt.chars().count() < 5000);
    }

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

/// Against a real database — see `crate::testdb`.
#[cfg(test)]
mod db_tests {
    use super::FollowUp;
    use crate::testdb;

    /// A summary pass is aichip asking, and anything a person does to the
    /// card outranks it: starting the card again drops the pending summary
    /// rather than refusing with "already running".
    #[tokio::test]
    async fn a_pending_summary_gives_way_to_a_person() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let orchestrator = t.orchestrator(dir.path());
        let (_, project) = t.project(dir.path(), false).await;
        let card = t.card(project, "silent").await;
        sqlx::query("UPDATE tasks SET board_column = 'review', worktree_path = $2 WHERE id = $1")
            .bind(card)
            .bind(dir.path().to_string_lossy().as_ref())
            .execute(&t.db.pool)
            .await
            .unwrap();
        let silent: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO runs (task_id, status, trigger, engine) VALUES ($1, 'completed', 'manual', 'mock')
             RETURNING id",
        )
        .bind(card)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();

        orchestrator.set_queue_paused(true).await.unwrap();
        let summary = orchestrator
            .enqueue_follow_up(card, FollowUp::Summarize { run_id: silent })
            .await
            .unwrap();
        // The person starts the card again while the summary is still queued.
        orchestrator
            .enqueue_task(card)
            .await
            .expect("the summary gives way");
        let status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id = $1")
            .bind(summary)
            .fetch_one(&t.db.pool)
            .await
            .unwrap();
        assert_eq!(status, "canceled");

        t.finish().await;
    }
}
