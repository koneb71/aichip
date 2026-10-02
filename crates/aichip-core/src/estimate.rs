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
