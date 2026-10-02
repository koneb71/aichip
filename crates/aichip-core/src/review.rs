//! A project's review policy: what a card must clear before Merge, and the
//! agent reviewer that reads each finished run's diff.
//!
//! Merge stays a person's click — nothing here lands anything. The policy
//! decides what that click requires, and the reviewer is one of those things:
//! a read-only pass in the card's worktree, by an agent that is not the one
//! that did the work, whose verdict comes back through `submit_review`.
//!
//! The loop is bounded: changes requested start **one** fix run per round,
//! through the same follow-up door a person's review note uses; at
//! `max_rounds` it stops and the card waits in the inbox for a person. A
//! reviewer that ends without a verdict counts as changes requested and stops
//! the loop too — fail closed, never "no news is good news".

use crate::db::Db;
use crate::runs::follow_up::FollowUp;
use crate::runs::orchestrator::Orchestrator;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Policy {
    pub require_checks: bool,
    pub require_review: bool,
    pub reviewer_agent_id: Option<Uuid>,
    pub max_rounds: i32,
    pub require_pr_green: bool,
    pub run_checks_after_every_run: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            require_checks: false,
            require_review: false,
            reviewer_agent_id: None,
            max_rounds: 2,
            require_pr_green: false,
            run_checks_after_every_run: false,
        }
    }
}

pub async fn policy(db: &Db, project_id: Uuid) -> anyhow::Result<Policy> {
    let row = sqlx::query(
        "SELECT require_checks, require_review, reviewer_agent_id, max_rounds, require_pr_green,
                run_checks_after_every_run
           FROM project_review_policy WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row
        .map(|r| Policy {
            require_checks: r.get("require_checks"),
            require_review: r.get("require_review"),
            reviewer_agent_id: r.get("reviewer_agent_id"),
            max_rounds: r.get("max_rounds"),
            require_pr_green: r.get("require_pr_green"),
            run_checks_after_every_run: r.get("run_checks_after_every_run"),
        })
        .unwrap_or_default())
}

/// When the card's work last changed: the latest finished run that did work
/// (not a summary, not a review). A check or a verdict older than this
/// judged something that is no longer the diff.
pub async fn last_work(db: &Db, task_id: Uuid) -> anyhow::Result<Option<DateTime<Utc>>> {
    Ok(sqlx::query_scalar(
        "SELECT max(finished_at) FROM runs
          WHERE task_id = $1 AND status = 'completed'
            AND trigger NOT IN ('summary', 'peer_review')",
    )
    .bind(task_id)
    .fetch_one(&db.pool)
    .await?)
}

/// Rounds of review since a person last started the card's work fresh.
pub async fn rounds(db: &Db, task_id: Uuid) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM review_decisions d
          WHERE d.task_id = $1
            AND d.created_at > COALESCE(
                (SELECT max(created_at) FROM runs
                  WHERE task_id = $1 AND trigger IN ('manual', 'resume', 'retry', 'handoff')),
                '-infinity'::timestamptz)",
    )
    .bind(task_id)
    .fetch_one(&db.pool)
    .await?)
}

/// `runs.trigger` of a review pass.
pub const PEER_REVIEW: &str = "peer_review";

/// Who asked for the review. A person's click gets one more round past
/// `max_rounds` — the cap bounds what agents do unattended, not a person.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Start {
    Automatic,
    Person,
}

/// Why a review did not start, said on the card.
#[derive(Debug, Clone, PartialEq)]
pub enum Skip {
    NoReviewer,
    OwnWork(String),
    CannotEnforce(String),
    RoundsSpent(i64),
}

impl Skip {
    pub fn sentence(&self) -> String {
        match self {
            Skip::NoReviewer => "This project requires a review but names no reviewer agent — set one in the project's review settings.".into(),
            Skip::OwnWork(name) => format!("{name} did this work, so {name} cannot review it. Pick a different reviewer, or review it yourself."),
            Skip::CannotEnforce(engine) => format!(
                "The reviewer runs on {engine}, which cannot be held to read-only — a review that could edit the diff it judges is not a review. Pick a reviewer on an engine that can."
            ),
            Skip::RoundsSpent(n) => format!("{n} rounds of review and the reviewer still asks for changes. Over to you."),
        }
    }
}

/// Start the review of the card's latest work, if the policy wants one and
/// can have one. Through `enqueue_follow_up`, so the agent and budget gates
/// apply as they do to every run.
pub async fn start(
    orch: &Orchestrator,
    task_id: Uuid,
    by: Start,
) -> anyhow::Result<Result<Uuid, Skip>> {
    let row = sqlx::query(
        "SELECT t.project_id, t.agent_id, COALESCE(a.engine, t.engine) AS engine
           FROM tasks t LEFT JOIN agents a ON a.id = t.agent_id WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_one(&orch.db.pool)
    .await?;
    let project: Uuid = row.get("project_id");
    let p = policy(&orch.db, project).await?;
    let Some(reviewer) = p.reviewer_agent_id else {
        return Ok(Err(Skip::NoReviewer));
    };
    let (name, engine): (String, Option<String>) =
        sqlx::query_as("SELECT name, engine FROM agents WHERE id = $1")
            .bind(reviewer)
            .fetch_one(&orch.db.pool)
            .await?;
    // The run that did the work, not only the card's assignee: a card
    // reassigned after its run must still not be reviewed by its author.
    let author: Option<Uuid> = sqlx::query_scalar(
        "SELECT COALESCE(r.agent_id, t.agent_id) FROM runs r JOIN tasks t ON t.id = r.task_id
          WHERE r.task_id = $1 AND r.trigger NOT IN ('summary', 'peer_review')
          ORDER BY r.created_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(&orch.db.pool)
    .await?
    .flatten();
    if author == Some(reviewer) || row.get::<Option<Uuid>, _>("agent_id") == Some(reviewer) {
        return Ok(Err(Skip::OwnWork(name)));
    }
    let engine = engine.unwrap_or_else(|| row.get("engine"));
    let enforces = orch
        .engine(&engine)
        .is_some_and(|e| e.capabilities().enforces_denied_tools);
    if !enforces {
        return Ok(Err(Skip::CannotEnforce(engine)));
    }
    let done = rounds(&orch.db, task_id).await?;
    if by == Start::Automatic && done >= p.max_rounds as i64 {
        return Ok(Err(Skip::RoundsSpent(done)));
    }
    let run = orch
        .enqueue_follow_up(
            task_id,
            FollowUp::Review {
                reviewer,
                round: done as i32 + 1,
            },
        )
        .await?;
    Ok(Ok(run))
}

/// One note in a verdict.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Note {
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<i32>,
    pub body: String,
}

pub const MAX_NOTES: usize = 20;
pub const MAX_NOTE_CHARS: usize = 1500;
pub const MAX_SUMMARY_CHARS: usize = 2000;

/// The verdict, checked. Pure, for the tests.
pub fn vet(verdict: &str, summary: &str, notes: &[Note]) -> Result<(), String> {
    if verdict != "approve" && verdict != "request_changes" {
        return Err("verdict is approve or request_changes".into());
    }
    if summary.chars().count() > MAX_SUMMARY_CHARS {
        return Err(format!(
            "keep the summary under {MAX_SUMMARY_CHARS} characters"
        ));
    }
    if notes.len() > MAX_NOTES {
        return Err(format!("at most {MAX_NOTES} notes — group the rest"));
    }
    if notes
        .iter()
        .any(|n| n.body.trim().is_empty() || n.body.chars().count() > MAX_NOTE_CHARS)
    {
        return Err(format!(
            "each note needs a body under {MAX_NOTE_CHARS} characters"
        ));
    }
    if verdict == "request_changes" && notes.is_empty() && summary.trim().is_empty() {
        return Err("say what to change".into());
    }
    Ok(())
}

/// Record a reviewer's verdict. Once per review run.
pub async fn submit(
    db: &Db,
    run_id: Uuid,
    verdict: &str,
    summary: &str,
    notes: &[Note],
) -> anyhow::Result<()> {
    vet(verdict, summary, notes).map_err(anyhow::Error::msg)?;
    let run = sqlx::query("SELECT task_id, agent_id, review_round FROM runs WHERE id = $1 AND trigger = 'peer_review'")
        .bind(run_id)
        .fetch_optional(&db.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("only a review run can submit a review"))?;
    let inserted = sqlx::query(
        "INSERT INTO review_decisions (task_id, run_id, reviewer_agent_id, round, verdict, summary, notes)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (run_id) WHERE run_id IS NOT NULL DO NOTHING",
    )
    .bind(run.get::<Uuid, _>("task_id"))
    .bind(run_id)
    .bind(run.get::<Option<Uuid>, _>("agent_id"))
    .bind(run.get::<Option<i32>, _>("review_round").unwrap_or(1))
    .bind(verdict)
    .bind(summary.trim())
    .bind(serde_json::to_value(notes)?)
    .execute(&db.pool)
    .await?;
    if inserted.rows_affected() == 0 {
        anyhow::bail!("this review already has a verdict");
    }
    Ok(())
}

/// The verdict as one comment on the card — what the fix run is asked to act
/// on, and what a person reads. Pure, for the tests.
pub fn as_comment(round: i32, verdict: &str, summary: &str, notes: &[Note]) -> String {
    let mut out = format!(
        "**Review, round {round}: {}**\n",
        if verdict == "approve" {
            "approved"
        } else {
            "changes requested"
        }
    );
    if !summary.trim().is_empty() {
        out.push_str(&format!("\n{}\n", summary.trim()));
    }
    for n in notes {
        let at = match (&n.file, n.line) {
            (Some(f), Some(l)) => format!("`{f}:{l}` — "),
            (Some(f), None) => format!("`{f}` — "),
            _ => String::new(),
        };
        out.push_str(&format!("\n- {at}{}", n.body.trim()));
    }
    out
}

impl Orchestrator {
    /// After a card's run completes: start the review a policy asks for, or —
    /// after a review — act on its verdict. Best-effort: anything that goes
    /// wrong is said on the card, never fails the run that already finished.
    ///
    /// Idempotent on purpose. It is called after every completed run of a
    /// card (work, fix, summary) and after its checks settle, and it starts a
    /// review only when no run of the card is live and no verdict already
    /// covers the latest work. So a silent run's summary pass, or a checks
    /// fix, is waited for rather than raced — their own completion calls
    /// this again.
    pub async fn settle_review(&self, task_id: Uuid, run_id: Uuid, trigger: &str) {
        if let Err(e) = self.settle_review_inner(task_id, run_id, trigger).await {
            tracing::warn!(%task_id, %run_id, error = %e, "could not settle the review");
        }
    }

    async fn settle_review_inner(
        &self,
        task_id: Uuid,
        run_id: Uuid,
        trigger: &str,
    ) -> anyhow::Result<()> {
        let project: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
            .bind(task_id)
            .fetch_one(&self.db.pool)
            .await?;
        let p = policy(&self.db, project).await?;
        if !p.require_review {
            return Ok(());
        }
        if trigger == PEER_REVIEW {
            return self.act_on_verdict(task_id, run_id, &p).await;
        }
        if !wants_review(&self.db, task_id).await? {
            return Ok(());
        }
        // Checks first, when the policy requires them: a review of work whose
        // checks are failing or still running is a round spent on the wrong
        // question. A green run, or none since this work, lets it through —
        // checks nobody started unasked are a person's to run.
        if p.require_checks && checks_hold(&self.db, task_id).await? {
            return Ok(());
        }
        let started = match start(self, task_id, Start::Automatic).await {
            Ok(started) => started,
            // A paused reviewer, a spent budget: the card says why it was
            // not reviewed, rather than only the server log.
            Err(e) => {
                let why = format!("The review could not start: {e}");
                crate::runs::report::post_system(&self.db, task_id, Some(run_id), &why).await?;
                return Ok(());
            }
        };
        match started {
            Ok(_) => Ok(()),
            // Said once, when the last round ended — not again after every
            // run a person starts by hand on the card.
            Err(Skip::RoundsSpent(_)) => Ok(()),
            Err(skip) => {
                crate::runs::report::post_system(&self.db, task_id, Some(run_id), &skip.sentence())
                    .await?;
                Ok(())
            }
        }
    }

    async fn act_on_verdict(&self, task_id: Uuid, run_id: Uuid, p: &Policy) -> anyhow::Result<()> {
        let decision = sqlx::query(
            "SELECT round, verdict, summary, notes FROM review_decisions WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_optional(&self.db.pool)
        .await?;
        let Some(d) = decision else {
            // Fail closed: no verdict is not an approval. Recorded as changes
            // requested, and the loop stops — there is nothing to fix.
            let round: Option<i32> =
                sqlx::query_scalar("SELECT review_round FROM runs WHERE id = $1")
                    .bind(run_id)
                    .fetch_one(&self.db.pool)
                    .await?;
            sqlx::query(
                "INSERT INTO review_decisions
                     (task_id, run_id, reviewer_agent_id, round, verdict, submitted, summary)
                 SELECT $1, $2, agent_id, $3, 'request_changes', FALSE, $4 FROM runs WHERE id = $2
                 ON CONFLICT (run_id) WHERE run_id IS NOT NULL DO NOTHING",
            )
            .bind(task_id)
            .bind(run_id)
            .bind(round.unwrap_or(1))
            .bind(NO_VERDICT)
            .execute(&self.db.pool)
            .await?;
            self.stop_for_a_person(
                task_id,
                run_id,
                "The reviewer ended without a verdict, so this card is not approved. Run the review again, or review it yourself.",
            )
            .await?;
            return Ok(());
        };
        let round: i32 = d.get("round");
        let verdict: String = d.get("verdict");
        let notes: Vec<Note> = serde_json::from_value(d.get("notes")).unwrap_or_default();
        let comment = as_comment(round, &verdict, &d.get::<String, _>("summary"), &notes);
        let reviewer: Option<Uuid> = sqlx::query_scalar("SELECT agent_id FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_one(&self.db.pool)
            .await?;
        let comment_id: Uuid = sqlx::query_scalar(
            "INSERT INTO task_comments (task_id, author, agent_id, content, run_id)
             VALUES ($1, 'agent', $2, $3, $4) RETURNING id",
        )
        .bind(task_id)
        .bind(reviewer)
        .bind(&comment)
        .bind(run_id)
        .fetch_one(&self.db.pool)
        .await?;
        if verdict == "approve" {
            return Ok(());
        }
        if round >= p.max_rounds {
            self.stop_for_a_person(task_id, run_id, &Skip::RoundsSpent(round as i64).sentence())
                .await?;
            return Ok(());
        }
        // One fix run for the whole round, acting on the verdict as a note.
        if let Err(e) = self
            .enqueue_follow_up(task_id, FollowUp::ReviewNote { comment_id })
            .await
        {
            crate::runs::report::post_system(
                &self.db,
                task_id,
                Some(run_id),
                &format!("The reviewer asked for changes, but the fix could not start: {e}"),
            )
            .await?;
        }
        Ok(())
    }

    /// The loop has stopped and a person has to look: said on the card, and
    /// sent wherever this machine sends things that wait on someone.
    async fn stop_for_a_person(
        &self,
        task_id: Uuid,
        run_id: Uuid,
        why: &str,
    ) -> anyhow::Result<()> {
        crate::runs::report::post_system(&self.db, task_id, Some(run_id), why).await?;
        let ctx = crate::attention::Ctx {
            title: "A review needs you".into(),
            body: why.to_string(),
            ..crate::attention::ctx_for_run(&self.db, run_id, None).await
        };
        crate::attention::fire(&self.db, crate::attention::Event::Review, ctx).await;
        crate::wake::raise(
            &self.db,
            task_id,
            Some(run_id),
            crate::wake::Kind::ReviewExhausted,
            "",
        )
        .await;
        Ok(())
    }
}

/// The latest checks of the card's latest work are running or did not pass.
async fn checks_hold(db: &Db, task_id: Uuid) -> anyhow::Result<bool> {
    let work = last_work(db, task_id).await?;
    let latest: Option<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT status, created_at FROM check_runs WHERE task_id = $1
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?;
    Ok(match (latest, work) {
        (Some((status, at)), Some(w)) => at > w && status != "passed",
        _ => false,
    })
}

/// The summary a reviewer that never called `submit_review` is recorded with.
pub const NO_VERDICT: &str = "The reviewer ended without a verdict.";

/// Whether the card's latest work still waits for a review: nothing of the
/// card is live (a summary or a checks fix is still to come and will ask
/// again), it sits in review, and no verdict is newer than the work.
async fn wants_review(db: &Db, task_id: Uuid) -> anyhow::Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT t.board_column = 'review'
            AND t.worktree_path IS NOT NULL
            AND NOT EXISTS (SELECT 1 FROM runs WHERE task_id = t.id
                               AND status NOT IN ('completed','failed','canceled'))
            AND EXISTS (SELECT 1 FROM runs WHERE task_id = t.id AND status = 'completed'
                           AND trigger NOT IN ('summary', 'peer_review'))
            AND COALESCE((SELECT max(created_at) FROM review_decisions WHERE task_id = t.id)
                           > (SELECT max(finished_at) FROM runs WHERE task_id = t.id
                                AND status = 'completed'
                                AND trigger NOT IN ('summary', 'peer_review')),
                         FALSE) = FALSE
           FROM tasks t WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?
    .unwrap_or(false))
}

/// What still stands between a card and Merge.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Unmet {
    pub kind: &'static str,
    pub message: String,
}

/// Every requirement of the policy the card does not meet now. Empty means
/// Merge may go ahead.
pub async fn gate(db: &Db, task_id: Uuid) -> anyhow::Result<Vec<Unmet>> {
    let project: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&db.pool)
        .await?;
    let p = policy(db, project).await?;
    let mut unmet = vec![];
    let work = last_work(db, task_id).await?;
    let newer = |at: Option<DateTime<Utc>>| match (at, work) {
        (Some(a), Some(w)) => a > w,
        (Some(_), None) => true,
        (None, _) => false,
    };
    if p.require_checks {
        let configured = crate::checks::config(db, project).await?.is_some();
        let latest = sqlx::query("SELECT status, created_at FROM check_runs WHERE task_id = $1 ORDER BY created_at DESC LIMIT 1")
            .bind(task_id)
            .fetch_optional(&db.pool)
            .await?;
        let message = match (configured, latest) {
            (false, _) => {
                Some("This project requires passing checks but has none set up.".to_string())
            }
            (true, None) => Some("The checks have not run on this card.".to_string()),
            (true, Some(r)) => {
                let status: String = r.get("status");
                if status != "passed" {
                    Some(format!(
                        "The latest checks {}.",
                        if status == "failed" {
                            "failed".to_string()
                        } else {
                            format!("are {status}")
                        }
                    ))
                } else if !newer(Some(r.get("created_at"))) {
                    Some(
                        "The checks passed on an earlier version of this work — run them again."
                            .to_string(),
                    )
                } else {
                    None
                }
            }
        };
        if let Some(message) = message {
            unmet.push(Unmet {
                kind: "checks",
                message,
            });
        }
    }
    if p.require_review {
        let latest = sqlx::query("SELECT verdict, created_at FROM review_decisions WHERE task_id = $1 ORDER BY created_at DESC LIMIT 1")
            .bind(task_id)
            .fetch_optional(&db.pool)
            .await?;
        let message = match latest {
            None => Some("No review has approved this card.".to_string()),
            Some(r) if r.get::<String, _>("verdict") != "approve" => {
                Some("The latest review asked for changes.".to_string())
            }
            Some(r) if !newer(Some(r.get("created_at"))) => {
                Some("The approval was for an earlier version of this work.".to_string())
            }
            Some(_) => None,
        };
        if let Some(message) = message {
            unmet.push(Unmet {
                kind: "review",
                message,
            });
        }
    }
    if p.require_pr_green {
        let (number, checks): (Option<i32>, Option<String>) =
            sqlx::query_as("SELECT pr_number, pr_checks FROM tasks WHERE id = $1")
                .bind(task_id)
                .fetch_one(&db.pool)
                .await?;
        let message = match (number, checks.as_deref()) {
            (None, _) => Some(
                "This project requires a green pull request, and this card has none.".to_string(),
            ),
            (Some(_), Some("passing")) => None,
            (Some(n), Some("none")) => Some(format!("Pull request #{n} has no checks to pass.")),
            (Some(n), Some(state)) => Some(format!("Pull request #{n}'s checks are {state}.")),
            (Some(n), None) => Some(format!("Pull request #{n}'s checks are not known yet.")),
        };
        if let Some(message) = message {
            unmet.push(Unmet {
                kind: "pull_request",
                message,
            });
        }
    }
    Ok(unmet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_must_be_one_of_two_and_say_something() {
        assert!(vet("approve", "", &[]).is_ok());
        assert!(vet("lgtm", "", &[]).is_err());
        assert!(
            vet("request_changes", "", &[]).is_err(),
            "changes, but which?"
        );
        assert!(vet("request_changes", "rename it", &[]).is_ok());
        let blank = Note {
            file: None,
            line: None,
            body: " ".into(),
        };
        assert!(vet("request_changes", "", &[blank]).is_err());
        let many = vec![
            Note {
                file: None,
                line: None,
                body: "x".into()
            };
            MAX_NOTES + 1
        ];
        assert!(vet("request_changes", "", &many).is_err());
    }

    #[test]
    fn a_verdict_reads_as_one_comment() {
        let notes = vec![
            Note {
                file: Some("src/a.rs".into()),
                line: Some(3),
                body: "unwrap here panics".into(),
            },
            Note {
                file: None,
                line: None,
                body: "add a test".into(),
            },
        ];
        let c = as_comment(2, "request_changes", "Close.", &notes);
        assert!(c.starts_with("**Review, round 2: changes requested**"));
        assert!(c.contains("- `src/a.rs:3` — unwrap here panics"));
        assert!(c.contains("- add a test"));
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
        card: Uuid,
        author: Uuid,
        reviewer: Uuid,
    }

    /// A card in review with one finished work run by `author`, in a project
    /// whose policy has `reviewer` review every run. The queue is paused, so
    /// what is started stays queued for the test to look at.
    async fn fixture(max_rounds: i32) -> Option<Fixture> {
        let t = testdb::fresh().await?;
        let dir = tempfile::tempdir().unwrap();
        let orch = t.orchestrator(dir.path());
        orch.set_queue_paused(true).await.unwrap();
        let (ws, project) = t.project(dir.path(), false).await;
        let card = t.card(project, "add a flag").await;
        let agent = |name: &'static str| {
            sqlx::query_scalar::<_, Uuid>(
                "INSERT INTO agents (workspace_id, name, engine) VALUES ($1, $2, 'mock') RETURNING id",
            )
            .bind(ws)
            .bind(name)
            .fetch_one(&t.db.pool)
        };
        let author = agent("Ada").await.unwrap();
        let reviewer = agent("Rex").await.unwrap();
        sqlx::query(
            "UPDATE tasks SET board_column = 'review', worktree_path = $2, agent_id = $3 WHERE id = $1",
        )
        .bind(card)
        .bind(dir.path().to_string_lossy().as_ref())
        .bind(author)
        .execute(&t.db.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_review_policy (project_id, require_review, reviewer_agent_id, max_rounds)
             VALUES ($1, TRUE, $2, $3)",
        )
        .bind(project)
        .bind(reviewer)
        .bind(max_rounds)
        .execute(&t.db.pool)
        .await
        .unwrap();
        let f = Fixture {
            t,
            orch,
            _dir: dir,
            project,
            card,
            author,
            reviewer,
        };
        f.work("manual").await;
        Some(f)
    }

    impl Fixture {
        /// A finished run of the card that did work.
        async fn work(&self, trigger: &str) -> Uuid {
            sqlx::query_scalar(
                "INSERT INTO runs (task_id, agent_id, status, trigger, engine, created_at, finished_at)
                 VALUES ($1, $2, 'completed', $3, 'mock', clock_timestamp(), clock_timestamp())
                 RETURNING id",
            )
            .bind(self.card)
            .bind(self.author)
            .bind(trigger)
            .fetch_one(&self.t.db.pool)
            .await
            .unwrap()
        }

        /// The one queued run of the card, which then "finishes".
        async fn finish_queued(&self) -> (Uuid, String) {
            let (id, trigger): (Uuid, String) = sqlx::query_as(
                "SELECT id, trigger FROM runs WHERE task_id = $1 AND status = 'queued'",
            )
            .bind(self.card)
            .fetch_one(&self.t.db.pool)
            .await
            .unwrap();
            sqlx::query("DELETE FROM queue WHERE run_id = $1")
                .bind(id)
                .execute(&self.t.db.pool)
                .await
                .unwrap();
            sqlx::query(
                "UPDATE runs SET status = 'completed', finished_at = clock_timestamp() WHERE id = $1",
            )
            .bind(id)
            .execute(&self.t.db.pool)
            .await
            .unwrap();
            (id, trigger)
        }

        async fn queued(&self) -> Vec<(String, Option<Uuid>, Option<i32>)> {
            sqlx::query_as(
                "SELECT trigger, agent_id, review_round FROM runs
                  WHERE task_id = $1 AND status = 'queued'",
            )
            .bind(self.card)
            .fetch_all(&self.t.db.pool)
            .await
            .unwrap()
        }

        async fn system_notes(&self) -> Vec<String> {
            sqlx::query_scalar(
                "SELECT content FROM task_comments WHERE task_id = $1 AND author = 'system'
                  ORDER BY created_at",
            )
            .bind(self.card)
            .fetch_all(&self.t.db.pool)
            .await
            .unwrap()
        }

        async fn inbox_reviews(&self) -> usize {
            let ws: Uuid = sqlx::query_scalar("SELECT workspace_id FROM projects WHERE id = $1")
                .bind(self.project)
                .fetch_one(&self.t.db.pool)
                .await
                .unwrap();
            crate::inbox::list(&self.t.db, ws, true)
                .await
                .unwrap()
                .iter()
                .filter(|i| i.kind == crate::inbox::Kind::Review)
                .count()
        }
    }

    #[tokio::test]
    async fn finished_work_is_reviewed_by_the_reviewer_not_its_author() {
        let Some(f) = fixture(2).await else { return };
        let work = f.work("manual").await;
        f.orch.settle_review(f.card, work, "manual").await;
        assert_eq!(
            f.queued().await,
            [(PEER_REVIEW.to_string(), Some(f.reviewer), Some(1))],
            "one review, as the reviewer, round 1"
        );
        // Settling again — a summary pass finishing, checks settling — does
        // not start a second one.
        f.orch.settle_review(f.card, work, "summary").await;
        assert_eq!(f.queued().await.len(), 1);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn the_author_never_reviews_its_own_work() {
        let Some(f) = fixture(2).await else { return };
        sqlx::query(
            "UPDATE project_review_policy SET reviewer_agent_id = $2 WHERE project_id = $1",
        )
        .bind(f.project)
        .bind(f.author)
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        let work = f.work("manual").await;
        f.orch.settle_review(f.card, work, "manual").await;
        assert!(f.queued().await.is_empty());
        assert!(f
            .system_notes()
            .await
            .iter()
            .any(|n| n.contains("cannot review it")));
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_reviewer_that_never_answers_has_not_approved() {
        let Some(f) = fixture(2).await else { return };
        let work = f.work("manual").await;
        f.orch.settle_review(f.card, work, "manual").await;
        let (review, trigger) = f.finish_queued().await;
        f.orch.settle_review(f.card, review, &trigger).await;

        let (verdict, submitted, by): (String, bool, Option<Uuid>) = sqlx::query_as(
            "SELECT verdict, submitted, reviewer_agent_id FROM review_decisions WHERE run_id = $1",
        )
        .bind(review)
        .fetch_one(&f.t.db.pool)
        .await
        .unwrap();
        assert_eq!((verdict.as_str(), submitted), ("request_changes", false));
        assert_eq!(by, Some(f.reviewer), "said whose review it was");
        // Nothing to fix, so nothing more runs; a person is told.
        assert!(f.queued().await.is_empty());
        assert_eq!(f.inbox_reviews().await, 1);
        let unmet = gate(&f.t.db, f.card).await.unwrap();
        assert_eq!(unmet.iter().map(|u| u.kind).collect::<Vec<_>>(), ["review"]);
        f.t.finish().await;
    }

    #[tokio::test]
    async fn changes_requested_fix_once_per_round_and_stop_at_the_cap() {
        let Some(f) = fixture(2).await else { return };
        let work = f.work("manual").await;
        f.orch.settle_review(f.card, work, "manual").await;

        for round in 1..=2 {
            let (review, trigger) = f.finish_queued().await;
            assert_eq!(trigger, PEER_REVIEW);
            submit(&f.t.db, review, "request_changes", "rename the flag", &[])
                .await
                .unwrap();
            // Once per run.
            assert!(submit(&f.t.db, review, "approve", "", &[]).await.is_err());
            f.orch.settle_review(f.card, review, &trigger).await;
            if round == 2 {
                break;
            }
            // One fix, acting on the verdict as a review note.
            assert_eq!(f.queued().await.len(), 1);
            let (fix, trigger) = f.finish_queued().await;
            assert_eq!(trigger, "review");
            f.orch.settle_review(f.card, fix, &trigger).await;
            assert_eq!(
                f.queued().await[0].2,
                Some(2),
                "the fix is reviewed, round 2"
            );
        }
        // Round 2 still wants changes: the cap holds, and a person is asked.
        assert!(f.queued().await.is_empty(), "nothing past max_rounds");
        assert_eq!(f.inbox_reviews().await, 1);
        let unmet = gate(&f.t.db, f.card).await.unwrap();
        assert_eq!(unmet[0].message, "The latest review asked for changes.");

        // A person's click gets one more round, past the cap.
        assert!(start(&f.orch, f.card, Start::Automatic)
            .await
            .unwrap()
            .is_err());
        start(&f.orch, f.card, Start::Person)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(f.queued().await[0].2, Some(3));
        assert_eq!(f.inbox_reviews().await, 0, "something is happening again");
        f.t.finish().await;
    }

    #[tokio::test]
    async fn an_approval_of_older_work_does_not_open_the_gate() {
        let Some(f) = fixture(2).await else { return };
        let work = f.work("manual").await;
        f.orch.settle_review(f.card, work, "manual").await;
        let (review, _) = f.finish_queued().await;
        submit(&f.t.db, review, "approve", "good", &[])
            .await
            .unwrap();
        assert!(gate(&f.t.db, f.card).await.unwrap().is_empty(), "approved");

        // The work changes after the approval.
        f.work("review").await;
        let unmet = gate(&f.t.db, f.card).await.unwrap();
        assert_eq!(
            unmet[0].message,
            "The approval was for an earlier version of this work."
        );
        f.t.finish().await;
    }

    #[tokio::test]
    async fn only_a_review_run_can_give_a_verdict() {
        let Some(f) = fixture(2).await else { return };
        let work = f.work("manual").await;
        let err = submit(&f.t.db, work, "approve", "", &[]).await.unwrap_err();
        assert!(err.to_string().contains("only a review run"), "{err}");
        f.t.finish().await;
    }

    #[tokio::test]
    async fn the_gate_names_every_requirement_left() {
        let Some(f) = fixture(2).await else { return };
        sqlx::query(
            "UPDATE project_review_policy SET require_checks = TRUE, require_pr_green = TRUE
              WHERE project_id = $1",
        )
        .bind(f.project)
        .execute(&f.t.db.pool)
        .await
        .unwrap();
        let unmet = gate(&f.t.db, f.card).await.unwrap();
        assert_eq!(
            unmet.iter().map(|u| u.kind).collect::<Vec<_>>(),
            ["checks", "review", "pull_request"]
        );
        // With no policy at all, nothing stands in the way.
        sqlx::query("DELETE FROM project_review_policy WHERE project_id = $1")
            .bind(f.project)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        assert!(gate(&f.t.db, f.card).await.unwrap().is_empty());
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_review_that_dies_still_ends_as_a_review() {
        let Some(f) = fixture(3).await else { return };
        let work = f.work("manual").await;
        f.orch.settle_review(f.card, work, "manual").await;
        // Round 1 fails before giving a verdict: recorded fail-closed.
        let (r1, _) = f.finish_queued().await;
        sqlx::query("UPDATE runs SET status = 'running', finished_at = NULL WHERE id = $1")
            .bind(r1)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        f.orch
            .finish(r1, aichip_shared::RunStatus::Failed, Some("exit 1".into()))
            .await
            .unwrap();
        let submitted: bool =
            sqlx::query_scalar("SELECT submitted FROM review_decisions WHERE run_id = $1")
                .bind(r1)
                .fetch_one(&f.t.db.pool)
                .await
                .unwrap();
        assert!(!submitted);
        assert_eq!(f.inbox_reviews().await, 1);

        // A person asks again; this one gives its verdict, then dies.
        start(&f.orch, f.card, Start::Person)
            .await
            .unwrap()
            .unwrap();
        let (r2, _) = f.finish_queued().await;
        sqlx::query("UPDATE runs SET status = 'running', finished_at = NULL WHERE id = $1")
            .bind(r2)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        submit(&f.t.db, r2, "request_changes", "rename it", &[])
            .await
            .unwrap();
        f.orch
            .finish(r2, aichip_shared::RunStatus::Failed, Some("exit 1".into()))
            .await
            .unwrap();
        assert_eq!(
            f.queued().await.first().map(|q| q.0.clone()),
            Some("review".to_string()),
            "its verdict is acted on: one fix run"
        );
        f.t.finish().await;
    }

    #[tokio::test]
    async fn a_dead_review_is_not_resumed_as_work() {
        let Some(f) = fixture(2).await else { return };
        let work = f.work("manual").await;
        f.orch.settle_review(f.card, work, "manual").await;
        let (review, _) = f.finish_queued().await;
        sqlx::query("UPDATE runs SET status = 'failed', session_id = 's', session_engine = 'mock' WHERE id = $1")
            .bind(review)
            .execute(&f.t.db.pool)
            .await
            .unwrap();
        let refused = crate::runs::resume::resume_dead_run(&f.orch, review)
            .await
            .unwrap_err();
        assert!(refused.to_string().contains("not resumed"), "{refused}");
        f.t.finish().await;
    }
}
