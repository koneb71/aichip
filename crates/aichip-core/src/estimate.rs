//! What something is likely to cost, from what similar things did.
//!
//! Two questions, both answered from history rather than from a price list:
//! what will this card cost to run (`for_task`), and when will this budget
//! run out at the rate it is going (`burn`). The first reads completed runs
//! — the same median-and-worst reading the team launch estimate has always
//! shown — and says how much history it stands on, because "$0.80 from 40
//! runs" and "$0.80 from 2" are not the same claim.

use chrono::{DateTime, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::budgets::{Cap, Standing};
use crate::db::Db;

/// Fewer similar runs than this, and the estimate widens its net.
const ENOUGH: i64 = 5;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub median_usd: f64,
    /// Nine in ten similar runs cost no more than this.
    pub p90_usd: f64,
    pub runs: i64,
    /// What "similar" ended up meaning: `project` (same project, tier and
    /// engine), `tier` (same tier and engine anywhere), or `engine`.
    pub basis: &'static str,
}

/// A card's likely cost, from completed, priced runs in the last 90 days —
/// as close to this card as there is enough history for. `None` when there
/// is no history worth quoting.
pub async fn for_task(
    db: &Db,
    project: Uuid,
    tier: Option<&str>,
    engine: &str,
) -> anyhow::Result<Option<Estimate>> {
    // Narrowest first; each arm is a literal, every value bound.
    let rungs: [(&'static str, &str); 3] = [
        (
            "project",
            "t.project_id = $1 AND r.tier_resolved IS NOT DISTINCT FROM $2 AND r.engine = $3",
        ),
        (
            "tier",
            "r.tier_resolved IS NOT DISTINCT FROM $2 AND r.engine = $3",
        ),
        ("engine", "r.engine = $3"),
    ];
    for (basis, filter) in rungs {
        let row = sqlx::query(&format!(
            "SELECT COUNT(*) AS runs,
                    percentile_cont(0.5) WITHIN GROUP (ORDER BY r.cost_usd) AS median,
                    percentile_cont(0.9) WITHIN GROUP (ORDER BY r.cost_usd) AS p90
               FROM runs r JOIN tasks t ON t.id = r.task_id
              WHERE r.status = 'completed' AND r.cost_usd IS NOT NULL
                AND r.created_at > now() - interval '90 days'
                AND {filter}"
        ))
        .bind(project)
        .bind(tier)
        .bind(engine)
        .fetch_one(&db.pool)
        .await?;
        let runs: i64 = row.get("runs");
        if runs >= ENOUGH {
            return Ok(Some(Estimate {
                median_usd: row.get::<Option<f64>, _>("median").unwrap_or(0.0),
                p90_usd: row.get::<Option<f64>, _>("p90").unwrap_or(0.0),
                runs,
                basis,
            }));
        }
    }
    Ok(None)
}

/// The same question for a card that exists: its project, the tier it asks
/// for and the engine that would run it.
pub async fn for_card(db: &Db, task_id: Uuid) -> anyhow::Result<Option<Estimate>> {
    let row = sqlx::query(
        "SELECT t.project_id, t.model_tier, COALESCE(a.engine, t.engine) AS engine
           FROM tasks t LEFT JOIN agents a ON a.id = t.agent_id WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_one(&db.pool)
    .await?;
    let tier: String = row.get("model_tier");
    // `auto` is settled per run, so any tier it has resolved to is evidence.
    let tier = (tier != "auto").then_some(tier);
    for_task(
        db,
        row.get("project_id"),
        tier.as_deref(),
        &row.get::<String, _>("engine"),
    )
    .await
}

/// When a budget will be spent at the rate it is going.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Burn {
    pub cap: Cap,
    /// Before the window turns; `None` means it lasts the window.
    pub runs_out_at: Option<DateTime<Utc>>,
}

pub fn burn(standing: &Standing) -> Option<Burn> {
    burn_at(standing, Utc::now())
}

/// Pure, so it is tested with a fixed clock. A straight line from the start
/// of the window through what is spent now — crude, and said to be: it is
/// "at this rate", not a prediction.
pub fn burn_at(standing: &Standing, now: DateTime<Utc>) -> Option<Burn> {
    let elapsed = (now - standing.window_start).num_seconds() as f64;
    if elapsed <= 0.0 {
        return None;
    }
    let p = &standing.policy;
    let u = &standing.used;
    let caps = [
        (Cap::Usd, p.cap_usd, u.usd),
        (
            Cap::Tokens,
            p.cap_output_tokens.map(|c| c as f64),
            u.output_tokens as f64,
        ),
        (Cap::Runs, p.cap_runs.map(f64::from), u.runs as f64),
    ];
    let mut soonest: Option<Burn> = None;
    for (cap, limit, used) in caps {
        let Some(limit) = limit else { continue };
        if used <= 0.0 {
            continue;
        }
        let rate = used / elapsed;
        let left = (limit - used).max(0.0);
        let at = now + chrono::Duration::seconds((left / rate) as i64);
        let runs_out_at = (at < standing.window_end).then_some(at);
        let sooner = match (&soonest, runs_out_at) {
            (None, _) => true,
            (
                Some(Burn {
                    runs_out_at: None, ..
                }),
                Some(_),
            ) => true,
            (
                Some(Burn {
                    runs_out_at: Some(s),
                    ..
                }),
                Some(a),
            ) => a < *s,
            _ => false,
        };
        if sooner {
            soonest = Some(Burn { cap, runs_out_at });
        }
    }
    soonest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budgets::{Policy, ScopeKind, Usage, Verdict, WindowKind};

    fn standing(
        cap_usd: f64,
        used_usd: f64,
        hours_in: i64,
        window_hours: i64,
    ) -> (Standing, DateTime<Utc>) {
        let start = DateTime::parse_from_rfc3339("2026-10-05T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        (
            Standing {
                policy: Policy {
                    id: Uuid::nil(),
                    name: "Weekly".into(),
                    scope_kind: ScopeKind::Machine,
                    scope_id: None,
                    window_kind: WindowKind::Week,
                    cap_usd: Some(cap_usd),
                    cap_output_tokens: None,
                    cap_runs: None,
                    warn_percent: 80,
                    stops_in_flight: false,
                    confirm_above_usd: None,
                    enabled: true,
                },
                used: Usage {
                    usd: used_usd,
                    output_tokens: 0,
                    runs: 0,
                },
                verdict: Verdict::Open,
                window_start: start,
                window_end: start + chrono::Duration::hours(window_hours),
            },
            start + chrono::Duration::hours(hours_in),
        )
    }

    #[test]
    fn at_this_rate_it_runs_out_before_the_window_turns() {
        // $20 of $40 in a day of a seven-day window: gone after day two.
        let (s, now) = standing(40.0, 20.0, 24, 168);
        let burn = burn_at(&s, now).unwrap();
        assert_eq!(burn.runs_out_at, Some(now + chrono::Duration::hours(24)));
    }

    #[test]
    fn a_slow_week_lasts_the_window() {
        let (s, now) = standing(40.0, 1.0, 24, 168);
        assert_eq!(burn_at(&s, now).unwrap().runs_out_at, None);
    }

    #[test]
    fn nothing_spent_forecasts_nothing() {
        let (s, now) = standing(40.0, 0.0, 24, 168);
        assert_eq!(burn_at(&s, now), None);
    }
}

/// Against a real database — see `crate::testdb`.
#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    /// History from six weeks ago: inside the estimate's 90 days, outside
    /// today's budget window.
    async fn history(t: &testdb::TestDb, card: Uuid, costs: &[f64]) {
        for cost in costs {
            sqlx::query(
                "INSERT INTO runs (task_id, status, trigger, engine, tier_resolved, cost_usd,
                                   created_at, started_at, finished_at)
                 VALUES ($1, 'completed', 'manual', 'mock', 'medium', $2,
                         now() - interval '40 days', now() - interval '40 days', now() - interval '40 days')",
            )
            .bind(card)
            .bind(cost)
            .execute(&t.db.pool)
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn an_estimate_widens_until_it_has_enough_history() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (_, here) = t.project(dir.path(), true).await;
        let (_, there) = t.project(&dir.path().join("there"), true).await;
        let fresh = t.card(here, "new").await;
        assert_eq!(
            for_card(&t.db, fresh).await.unwrap(),
            None,
            "no history, no number"
        );

        // Six runs elsewhere: not enough here, so it reads the tier anywhere.
        let old = t.card(there, "old").await;
        history(&t, old, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).await;
        let e = for_card(&t.db, fresh).await.unwrap().unwrap();
        assert_eq!((e.basis, e.runs), ("tier", 6));
        assert!((e.median_usd - 3.5).abs() < 1e-9 && e.p90_usd > 5.0);

        // Five of its own: now it can speak for this project.
        let mine = t.card(here, "mine").await;
        history(&t, mine, &[0.1, 0.1, 0.1, 0.1, 0.1]).await;
        assert_eq!(
            for_card(&t.db, fresh).await.unwrap().unwrap().basis,
            "project"
        );
        t.finish().await;
    }

    /// A start that could overrun what is left asks first; acknowledged, it
    /// goes ahead and the choice is on record.
    #[tokio::test]
    async fn a_start_that_could_overrun_a_budget_asks_first() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (_, project) = t.project(dir.path(), true).await;
        let old = t.card(project, "old").await;
        history(&t, old, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).await;
        let card = t.card(project, "new").await;
        let policy: Uuid = sqlx::query_scalar(
            "INSERT INTO budget_policies (name, scope_kind, window_kind, cap_usd, confirm_above_usd)
             VALUES ('Daily', 'machine', 'day', 10, 1) RETURNING id",
        )
        .fetch_one(&t.db.pool)
        .await
        .unwrap();

        // $10 left and a likely worst case near $5.50: nothing to ask.
        assert_eq!(
            crate::budgets::forecast_check(&t.db, card, false)
                .await
                .unwrap(),
            Ok(())
        );

        sqlx::query("UPDATE budget_policies SET cap_usd = 4 WHERE id = $1")
            .bind(policy)
            .execute(&t.db.pool)
            .await
            .unwrap();
        let ask = crate::budgets::forecast_check(&t.db, card, false)
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!((ask.policy.as_str(), ask.headroom_usd), ("Daily", 4.0));
        assert!(ask.to_string().contains("only $4.00 is left"));

        assert_eq!(
            crate::budgets::forecast_check(&t.db, card, true)
                .await
                .unwrap(),
            Ok(())
        );
        let acks: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM budget_incidents WHERE policy_id = $1 AND kind = 'forecast_ack'",
        )
        .bind(policy)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        assert_eq!(acks, 1);
        t.finish().await;
    }
}
