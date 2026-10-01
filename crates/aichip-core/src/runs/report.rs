//! What a run says on its card when it finishes.
//!
//! A run used to end in silence: the card moved to Review, its account of what
//! it did went into the agent's private memory and nowhere a person reading
//! the card would look. Now every completed run leaves a short report in the
//! card's thread, headed by why the run happened, so the thread reads as the
//! card's history — the work, the fix for the review note, the conflict
//! resolution — in order.

use crate::db::Db;
use crate::runs::orchestrator::clip_chars;
use uuid::Uuid;

/// How much of a run's final message a report keeps. Enough for any summary
/// an agent should write; a transcript pasted as its last message is clipped,
/// and the full thing is one click away in the run's history.
pub const MAX_REPORT_CHARS: usize = 4000;

/// The report's first line, from why the run exists (`runs.trigger`).
pub fn header(trigger: &str, variant: Option<&str>) -> String {
    match (trigger, variant) {
        (_, Some(label)) => format!("Bake-off variant {label}"),
        ("resume", _) => "Work report (resumed)".into(),
        ("review", _) => "Fix for a review note".into(),
        ("checks", _) => "Fix for failing checks".into(),
        ("conflict", _) => "Conflict resolution".into(),
        ("summary", _) => "Summary".into(),
        _ => "Work report".into(),
    }
}

/// What the run said, clipped — or that it said nothing.
pub fn body(output: &str) -> String {
    let body = output.trim();
    if body.is_empty() {
        "(no summary)".to_string()
    } else {
        clip_chars(body, MAX_REPORT_CHARS)
    }
}

/// The comment's text: the header, then what the run said.
pub fn render(trigger: &str, variant: Option<&str>, output: &str) -> String {
    format!("**{}**\n\n{}", header(trigger, variant), body(output))
}

/// Post a run's report on its card, as the agent that ran it.
pub async fn post(
    db: &Db,
    task_id: Uuid,
    run_id: Uuid,
    agent_id: Option<Uuid>,
    trigger: &str,
    variant: Option<&str>,
    output: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO task_comments (task_id, author, agent_id, content, run_id)
         VALUES ($1, 'agent', $2, $3, $4)",
    )
    .bind(task_id)
    .bind(agent_id)
    .bind(render(trigger, variant, output))
    .bind(run_id)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// A line from aichip itself — not an agent, not a person — such as how a
/// card's checks went.
pub async fn post_system(
    db: &Db,
    task_id: Uuid,
    run_id: Option<Uuid>,
    text: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO task_comments (task_id, author, content, run_id)
         VALUES ($1, 'system', $2, $3)",
    )
    .bind(task_id)
    .bind(text)
    .bind(run_id)
    .execute(&db.pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_headed_by_why_the_run_happened() {
        assert!(render("manual", None, "Added CSV export.").starts_with("**Work report**"));
        assert!(render("review", None, "x").starts_with("**Fix for a review note**"));
        assert!(render("checks", None, "x").starts_with("**Fix for failing checks**"));
        assert!(render("conflict", None, "x").starts_with("**Conflict resolution**"));
        assert!(render("bakeoff", Some("B"), "x").starts_with("**Bake-off variant B**"));
        assert!(render("manual", None, "Added CSV export.").ends_with("Added CSV export."));
    }

    #[test]
    fn a_silent_run_says_so() {
        assert!(render("manual", None, "  \n").ends_with("(no summary)"));
    }

    #[test]
    fn a_transcript_pasted_as_the_last_message_is_clipped() {
        let long = "x".repeat(MAX_REPORT_CHARS * 3);
        let report = render("manual", None, &long);
        assert!(report.chars().count() < MAX_REPORT_CHARS + 100);
        assert!(report.ends_with('…'));
    }
}
