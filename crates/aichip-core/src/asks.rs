//! A card's agent asking a person something.
//!
//! The chat assistant has had `ask_user` for a while; a card's run had only
//! the permission prompt, so an agent that hit a genuine fork — "keep the old
//! API or break it?" — guessed, or wrote the question into its summary where
//! nothing would ever answer it.
//!
//! The run does not wait. Holding a slot for however long a person takes is
//! what permission prompts already cost, and a question is not that urgent:
//! the agent finishes its turn, the card goes to review with the question on
//! it, and the answer comes back as a follow-up in the same worktree with the
//! same session — `FollowUp::Answer`, through the same door every follow-up
//! uses, so the agent and budget gates apply.

use crate::approvals::Refusal;
use crate::db::Db;
use crate::runs::follow_up::FollowUp;
use crate::runs::orchestrator::Orchestrator;
use uuid::Uuid;

pub const MAX_QUESTION_CHARS: usize = 600;
pub const MAX_OPTIONS: usize = 5;
pub const MAX_OPTION_CHARS: usize = 80;

/// The question and its suggested answers, checked. Pure, for the tests.
pub fn vet(question: &str, options: &[String]) -> Result<(String, Vec<String>), String> {
    let question = question.trim();
    if question.is_empty() {
        return Err("ask something — the question is empty".into());
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        return Err(format!(
            "keep the question under {MAX_QUESTION_CHARS} characters"
        ));
    }
    if options.len() > MAX_OPTIONS {
        return Err(format!(
            "offer at most {MAX_OPTIONS} answers to choose from"
        ));
    }
    let mut kept = Vec::new();
    for o in options {
        let o = o.trim();
        if o.is_empty() {
            continue;
        }
        if o.chars().count() > MAX_OPTION_CHARS {
            return Err(format!(
                "keep each suggested answer under {MAX_OPTION_CHARS} characters"
            ));
        }
        kept.push(o.to_string());
    }
    Ok((question.to_string(), kept))
}

/// Record the question. One open question per card: a newer one replaces the
/// older, which the agent asking again has already made moot.
pub async fn ask(
    db: &Db,
    run_id: Uuid,
    task_id: Uuid,
    question: &str,
    options: &[String],
) -> anyhow::Result<Uuid> {
    let (question, options) = vet(question, options).map_err(anyhow::Error::msg)?;
    let mut tx = db.pool.begin().await?;
    sqlx::query(
        "UPDATE run_questions SET answered_at = now(), answer = NULL
          WHERE task_id = $1 AND answered_at IS NULL",
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await?;
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO run_questions (run_id, task_id, question, options)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(run_id)
    .bind(task_id)
    .bind(&question)
    .bind(serde_json::to_value(&options)?)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    let ctx = crate::attention::Ctx {
        title: "aichip: an agent asked you something".to_string(),
        body: question.clone(),
        ..crate::attention::ctx_for_run(db, run_id, None).await
    };
    crate::attention::fire(db, crate::attention::Event::Question, ctx).await;
    Ok(id)
}

/// Answer it, and send the answer back to work.
///
/// If the follow-up cannot start — the card is running again, its worktree is
/// gone, its agent is paused — the answer is not kept: a recorded answer with
/// no run behind it would vanish from the inbox while nothing acted on it.
pub async fn answer(orch: &Orchestrator, question_id: Uuid, answer: &str) -> Result<Uuid, Refusal> {
    let answer = answer.trim();
    if answer.is_empty() {
        return Err(Refusal::Invalid(
            "write an answer — an empty one tells the agent nothing".into(),
        ));
    }
    let task_id: Uuid = sqlx::query_scalar(
        "UPDATE run_questions SET answer = $2, answered_at = now()
          WHERE id = $1 AND answered_at IS NULL
        RETURNING task_id",
    )
    .bind(question_id)
    .bind(answer)
    .fetch_optional(&orch.db.pool)
    .await?
    .ok_or_else(|| Refusal::Conflict("that question has already been answered".into()))?;

    match orch
        .enqueue_follow_up(task_id, FollowUp::Answer { question_id })
        .await
    {
        Ok(run_id) => {
            sqlx::query("UPDATE run_questions SET answer_run_id = $2 WHERE id = $1")
                .bind(question_id)
                .bind(run_id)
                .execute(&orch.db.pool)
                .await?;
            // On the card's thread too, so the conversation reads whole there.
            sqlx::query(
                "INSERT INTO task_comments (task_id, author, content) VALUES ($1, 'user', $2)",
            )
            .bind(task_id)
            .bind(format!("**Answer:** {answer}"))
            .execute(&orch.db.pool)
            .await?;
            Ok(run_id)
        }
        Err(e) => {
            sqlx::query("UPDATE run_questions SET answer = NULL, answered_at = NULL WHERE id = $1")
                .bind(question_id)
                .execute(&orch.db.pool)
                .await?;
            Err(Refusal::Gated(e))
        }
    }
}

/// Close it without an answer: the person decided it no longer matters.
pub async fn dismiss(db: &Db, question_id: Uuid) -> Result<(), Refusal> {
    let done = sqlx::query(
        "UPDATE run_questions SET answered_at = now() WHERE id = $1 AND answered_at IS NULL",
    )
    .bind(question_id)
    .execute(&db.pool)
    .await?;
    if done.rows_affected() == 0 {
        return Err(Refusal::Conflict(
            "that question has already been answered".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_must_say_something_and_not_say_too_much() {
        assert!(vet("  ", &[]).is_err());
        assert!(vet(&"x".repeat(MAX_QUESTION_CHARS + 1), &[]).is_err());
        let (q, o) = vet(" keep it? ", &["yes".into(), "  ".into(), "no".into()]).unwrap();
        assert_eq!(
            (q.as_str(), o),
            ("keep it?", vec!["yes".to_string(), "no".to_string()])
        );
        assert!(vet("q", &vec!["a".to_string(); MAX_OPTIONS + 1]).is_err());
        assert!(vet("q", &["x".repeat(MAX_OPTION_CHARS + 1)]).is_err());
    }
}
