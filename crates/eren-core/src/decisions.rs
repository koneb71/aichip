//! Something an agent proposes and only a person can do.
//!
//! An agent can see that a card should start, or move, or wait for another —
//! and the rule every agent toolbox here holds is that it may not do those
//! things itself. Before this it could only say so in prose. A proposal is
//! that prose made actionable: one of a closed set of effects, a reason, and a
//! row in the inbox that a person approves or turns down.
//!
//! Approving runs the effect through the same function the matching button
//! does (the server owns that dispatch, since those functions are route
//! handlers), so a proposal can never do what a click could not.

use crate::db::Db;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a proposal would do. Closed: an agent picks one of these or nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    /// Start a card, exactly as its Start button would.
    StartCard { card_id: Uuid },
    /// Move a card to backlog, review or done. Never to "running" — that is
    /// `StartCard`, which goes through the start checks.
    MoveCard { card_id: Uuid, column: String },
    /// Hand a card to an agent, by name.
    AssignCard { card_id: Uuid, agent: String },
    /// Make one card wait for another to land.
    AddBlocker { card_id: Uuid, blocked_by: Uuid },
    /// Pause an agent, by name.
    PauseAgent { agent: String },
}

const COLUMNS: [&str; 3] = ["backlog", "review", "done"];

/// Open proposals one run may leave. Enough to raise what matters; not
/// enough to fill the inbox with a run's every thought.
pub const MAX_OPEN_PER_RUN: i64 = 5;
pub const MAX_REASON_CHARS: usize = 500;

impl Effect {
    /// The checks that need no database. Pure, for the tests.
    pub fn vet(&self) -> Result<(), String> {
        match self {
            Effect::MoveCard { column, .. } if !COLUMNS.contains(&column.as_str()) => Err(format!(
                "a card can be moved to {} — to start one, propose start_card",
                COLUMNS.join(", ")
            )),
            Effect::AssignCard { agent, .. } | Effect::PauseAgent { agent }
                if agent.trim().is_empty() =>
            {
                Err("name the agent".into())
            }
            Effect::AddBlocker {
                card_id,
                blocked_by,
            } if card_id == blocked_by => Err("a card cannot wait for itself".into()),
            _ => Ok(()),
        }
    }

    /// The cards this effect names, which must all be in the proposer's workspace.
    pub fn cards(&self) -> Vec<Uuid> {
        match self {
            Effect::StartCard { card_id }
            | Effect::MoveCard { card_id, .. }
            | Effect::AssignCard { card_id, .. } => vec![*card_id],
            Effect::AddBlocker {
                card_id,
                blocked_by,
            } => vec![*card_id, *blocked_by],
            Effect::PauseAgent { .. } => vec![],
        }
    }

    pub fn agent(&self) -> Option<&str> {
        match self {
            Effect::AssignCard { agent, .. } | Effect::PauseAgent { agent } => Some(agent.trim()),
            _ => None,
        }
    }
}

/// One line saying what approving would do, with titles rather than ids.
pub async fn describe(db: &Db, effect: &Effect) -> String {
    let title = |id: Uuid| async move {
        sqlx::query_scalar::<_, String>("SELECT title FROM tasks WHERE id = $1")
            .bind(id)
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten()
            .map(|t| format!("“{t}”"))
            .unwrap_or_else(|| "a card that no longer exists".into())
    };
    match effect {
        Effect::StartCard { card_id } => format!("Start {}", title(*card_id).await),
        Effect::MoveCard { card_id, column } => {
            format!("Move {} to {column}", title(*card_id).await)
        }
        Effect::AssignCard { card_id, agent } => {
            format!("Give {} to {agent}", title(*card_id).await)
        }
        Effect::AddBlocker {
            card_id,
            blocked_by,
        } => format!(
            "Make {} wait for {}",
            title(*card_id).await,
            title(*blocked_by).await
        ),
        Effect::PauseAgent { agent } => format!("Pause {agent}"),
    }
}

/// Record a proposal, after checking it names real things in this workspace.
pub async fn propose(
    db: &Db,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    run_id: Option<Uuid>,
    proposed_by: &str,
    effect: &Effect,
    reason: &str,
) -> anyhow::Result<Uuid> {
    effect.vet().map_err(anyhow::Error::msg)?;
    let reason = reason.trim();
    if reason.is_empty() {
        anyhow::bail!("say why — a person approves the reason as much as the change");
    }
    if reason.chars().count() > MAX_REASON_CHARS {
        anyhow::bail!("keep the reason under {MAX_REASON_CHARS} characters");
    }
    for card in effect.cards() {
        let here: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tasks t JOIN projects p ON p.id = t.project_id
                             WHERE t.id = $1 AND p.workspace_id = $2)",
        )
        .bind(card)
        .bind(workspace_id)
        .fetch_one(&db.pool)
        .await?;
        if !here {
            anyhow::bail!("there is no card {card} in this workspace");
        }
    }
    if let Some(agent) = effect.agent() {
        let known: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM agents WHERE workspace_id = $1
                             AND lower(name) = lower($2) AND status <> 'retired')",
        )
        .bind(workspace_id)
        .bind(agent)
        .fetch_one(&db.pool)
        .await?;
        if !known {
            anyhow::bail!("there is no agent called {agent} in this workspace");
        }
    }
    if let Some(run) = run_id {
        let open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM decisions WHERE run_id = $1 AND status = 'open'",
        )
        .bind(run)
        .fetch_one(&db.pool)
        .await?;
        if open >= MAX_OPEN_PER_RUN {
            anyhow::bail!("that is {MAX_OPEN_PER_RUN} open proposals from this run — say the rest in your summary");
        }
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO decisions (workspace_id, project_id, run_id, proposed_by, effect, reason)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(workspace_id)
    .bind(project_id)
    .bind(run_id)
    .bind(proposed_by)
    .bind(serde_json::to_value(effect)?)
    .bind(reason)
    .fetch_one(&db.pool)
    .await?;

    let mut ctx = match run_id {
        Some(run) => crate::attention::ctx_for_run(db, run, None).await,
        None => crate::attention::Ctx {
            title: String::new(),
            body: String::new(),
            project: None,
            card: None,
            tool: None,
            run_id: None,
            url: None,
        },
    };
    ctx.title = "eren: an agent proposed a decision".into();
    ctx.body = format!("{} — {reason}", describe(db, effect).await);
    crate::attention::fire(db, crate::attention::Event::Decision, ctx).await;
    Ok(id)
}

/// An open proposal, claimed for deciding. `None` if it is no longer open —
/// the claim is the status write itself, so a double click decides once.
pub async fn claim(db: &Db, id: Uuid, status: &str) -> anyhow::Result<Option<Effect>> {
    let effect: Option<serde_json::Value> = sqlx::query_scalar(
        "UPDATE decisions SET status = $2, decided_at = now()
          WHERE id = $1 AND status = 'open' RETURNING effect",
    )
    .bind(id)
    .bind(status)
    .fetch_optional(&db.pool)
    .await?;
    Ok(match effect {
        Some(v) => Some(serde_json::from_value(v)?),
        None => None,
    })
}

/// Put a claimed proposal back, untouched, for a refusal the person can
/// answer — the card is blocked for now, the start wants its cost
/// acknowledged. Only a claim nothing has settled: an outcome on the row means
/// the effect ran and is on the record.
pub async fn reopen(db: &Db, id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE decisions SET status = 'open', decided_at = NULL
          WHERE id = $1 AND status = 'approved' AND outcome IS NULL",
    )
    .bind(id)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// What happened when an approved effect ran. A failure is recorded, not
/// hidden: "approved" that did nothing would be a lie on the record.
pub async fn settle(db: &Db, id: Uuid, result: Result<String, String>) -> anyhow::Result<()> {
    let (status, outcome) = match result {
        Ok(o) => ("approved", o),
        Err(e) => ("failed", e),
    };
    sqlx::query("UPDATE decisions SET status = $2, outcome = $3 WHERE id = $1")
        .bind(id)
        .bind(status)
        .bind(outcome)
        .execute(&db.pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_listed_effects_parse() {
        let ok: Effect =
            serde_json::from_value(json!({"kind": "start_card", "card_id": Uuid::nil()})).unwrap();
        assert_eq!(
            ok,
            Effect::StartCard {
                card_id: Uuid::nil()
            }
        );
        for bad in [
            json!({"kind": "merge_card", "card_id": Uuid::nil()}),
            json!({"kind": "set_setting", "key": "x"}),
            json!({"kind": "start_card"}),
        ] {
            assert!(
                serde_json::from_value::<Effect>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_move_is_never_a_way_to_start() {
        let id = Uuid::new_v4();
        assert!(Effect::MoveCard {
            card_id: id,
            column: "running".into()
        }
        .vet()
        .is_err());
        assert!(Effect::MoveCard {
            card_id: id,
            column: "review".into()
        }
        .vet()
        .is_ok());
        assert!(Effect::AddBlocker {
            card_id: id,
            blocked_by: id
        }
        .vet()
        .is_err());
        assert!(Effect::PauseAgent { agent: " ".into() }.vet().is_err());
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    async fn status(db: &Db, id: Uuid) -> String {
        sqlx::query_scalar("SELECT status FROM decisions WHERE id = $1")
            .bind(id)
            .fetch_one(&db.pool)
            .await
            .unwrap()
    }

    /// A refusal the person can answer puts the proposal back; one that ran
    /// stays on the record; a restart mid-approval puts it back too.
    #[tokio::test]
    async fn a_claim_that_did_not_run_goes_back_to_the_inbox() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, project) = t.project(dir.path(), false).await;
        let card = t.card(project, "c").await;
        let effect = Effect::StartCard { card_id: card };
        let id = propose(
            &t.db,
            ws,
            Some(project),
            None,
            "Ada",
            &effect,
            "it is ready",
        )
        .await
        .unwrap();

        claim(&t.db, id, "approved").await.unwrap().unwrap();
        reopen(&t.db, id).await.unwrap();
        assert_eq!(status(&t.db, id).await, "open");

        // Settled: the effect ran, and nothing puts it back.
        claim(&t.db, id, "approved").await.unwrap().unwrap();
        settle(&t.db, id, Ok("started".into())).await.unwrap();
        reopen(&t.db, id).await.unwrap();
        assert_eq!(status(&t.db, id).await, "approved");

        // Claimed, then the server went down before the effect reported.
        let lost = propose(&t.db, ws, Some(project), None, "Ada", &effect, "again")
            .await
            .unwrap();
        claim(&t.db, lost, "approved").await.unwrap().unwrap();
        let orch = t.orchestrator(dir.path());
        orch.recover_orphans().await.unwrap();
        assert_eq!(status(&t.db, lost).await, "open");
        assert_eq!(status(&t.db, id).await, "approved", "a settled one stays");
        t.finish().await;
    }
}
