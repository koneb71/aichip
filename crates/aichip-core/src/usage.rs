//! What the user's plan has left, as their own CLI reports it.
//!
//! aichip asks Anthropic nothing and holds no credential. Claude Code already
//! prints `rate_limit_event` on its own stdout as it works; this keeps what it
//! says instead of discarding it. Same relationship as everything else here:
//! read the binary's output, never its configuration.
//!
//! Before this, the healthy telemetry was thrown away and the *first* thing
//! anyone learned about their usage was a run that failed. Knowing you are at
//! the edge is only useful while you can still act on it.

use crate::db::Db;
use chrono::{DateTime, Utc};
use sqlx::Row;

/// One limit's current position.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limit {
    pub engine: String,
    /// `five_hour`, `seven_day` — the CLI's own words, not ours.
    pub limit_type: String,
    /// `allowed` | `warning` | `blocked`.
    pub status: String,
    pub resets_at: Option<DateTime<Utc>>,
    pub using_overage: bool,
    pub updated_at: DateTime<Utc>,
}

/// Whether this observation is worth keeping in the history.
///
/// The CLI prints `rate_limit_event` continuously while a run works, so nearly
/// every one of them says exactly what the last one said. A row per ping would
/// be thousands a day carrying no information; a row per *transition* is the
/// same history at a readable size.
///
/// A new window counts as a transition even at the same status, because
/// "allowed again, resets Thursday" is the week turning over — the single most
/// useful thing in the log, and invisible if only the status is compared.
pub fn is_transition(
    previous: Option<(&str, Option<DateTime<Utc>>)>,
    status: &str,
    resets_at: Option<DateTime<Utc>>,
) -> bool {
    match previous {
        // Never heard from before: always worth the first row.
        None => true,
        Some((was, window)) => was != status || window != resets_at,
    }
}

/// Record what a run just heard. Called for every `rate_limit_event`.
///
/// Two writes with different jobs: `usage_limits` is upserted so it always
/// holds the current position, and `usage_events` gains a row only when
/// [`is_transition`] says something actually changed.
pub async fn record(
    db: &Db,
    engine: &str,
    limit_type: &str,
    status: &str,
    resets_at: Option<DateTime<Utc>>,
    using_overage: bool,
) -> anyhow::Result<()> {
    // Read before the upsert overwrites it — this is the only moment the
    // previous state still exists.
    let before = sqlx::query(
        "SELECT status, resets_at FROM usage_limits WHERE engine = $1 AND limit_type = $2",
    )
    .bind(engine)
    .bind(limit_type)
    .fetch_optional(&db.pool)
    .await?;
    let before: Option<(String, Option<DateTime<Utc>>)> =
        before.map(|r| (r.get("status"), r.get("resets_at")));

    if is_transition(
        before.as_ref().map(|(s, w)| (s.as_str(), *w)),
        status,
        resets_at,
    ) {
        sqlx::query(
            "INSERT INTO usage_events
                 (engine, limit_type, status, previous, resets_at, using_overage)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(engine)
        .bind(limit_type)
        .bind(status)
        .bind(before.as_ref().map(|(s, _)| s.as_str()))
        .bind(resets_at)
        .bind(using_overage)
        .execute(&db.pool)
        .await?;
    }

    sqlx::query(
        "INSERT INTO usage_limits
             (engine, limit_type, status, resets_at, using_overage, updated_at)
         VALUES ($1, $2, $3, $4, $5, now())
         ON CONFLICT (engine, limit_type) DO UPDATE
            SET status = EXCLUDED.status,
                resets_at = EXCLUDED.resets_at,
                using_overage = EXCLUDED.using_overage,
                updated_at = now()",
    )
    .bind(engine)
    .bind(limit_type)
    .bind(status)
    .bind(resets_at)
    .bind(using_overage)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Every limit we have heard about, worst first.
///
/// Ordered by severity rather than by name because the reason to look at this
/// is to find the one that is about to stop you.
pub async fn all(db: &Db) -> anyhow::Result<Vec<Limit>> {
    let rows = sqlx::query(
        "SELECT engine, limit_type, status, resets_at, using_overage, updated_at
           FROM usage_limits
          ORDER BY CASE status WHEN 'blocked' THEN 0 WHEN 'warning' THEN 1 ELSE 2 END,
                   limit_type",
    )
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Limit {
            engine: r.get("engine"),
            limit_type: r.get("limit_type"),
            status: r.get("status"),
            resets_at: r.get("resets_at"),
            using_overage: r.get("using_overage"),
            updated_at: r.get("updated_at"),
        })
        .collect())
}

/// One recorded change of state.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub engine: String,
    pub limit_type: String,
    pub status: String,
    /// What it was before. `None` is the first time this limit was heard from.
    pub previous: Option<String>,
    pub resets_at: Option<DateTime<Utc>>,
    pub using_overage: bool,
    pub observed_at: DateTime<Utc>,
}

/// How a limit has behaved over a window of days.
///
/// `days_seen` rather than a rate: aichip only hears about a limit while a run
/// is working, so "3 of the 9 days you actually ran anything" is a fact, and
/// "33% of the time" would be an inference about the days it heard nothing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pattern {
    pub limit_type: String,
    /// Days on which this limit was heard from at all.
    pub days_seen: i64,
    /// Days on which it warned or blocked.
    pub days_pinched: i64,
    /// Times it stopped a run outright.
    pub times_blocked: i64,
}

/// Recent transitions, newest first.
pub async fn history(db: &Db, days: i64) -> anyhow::Result<Vec<Event>> {
    let rows = sqlx::query(
        "SELECT engine, limit_type, status, previous, resets_at, using_overage, observed_at
           FROM usage_events
          WHERE observed_at > now() - make_interval(days => $1::int)
          ORDER BY observed_at DESC
          LIMIT 200",
    )
    .bind(days as i32)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Event {
            engine: r.get("engine"),
            limit_type: r.get("limit_type"),
            status: r.get("status"),
            previous: r.get("previous"),
            resets_at: r.get("resets_at"),
            using_overage: r.get("using_overage"),
            observed_at: r.get("observed_at"),
        })
        .collect())
}

/// Whether each limit is a wall you meet often, or met once.
pub async fn patterns(db: &Db, days: i64) -> anyhow::Result<Vec<Pattern>> {
    let rows = sqlx::query(
        "SELECT limit_type,
                COUNT(DISTINCT observed_at::date) AS days_seen,
                COUNT(DISTINCT observed_at::date)
                    FILTER (WHERE status <> 'allowed') AS days_pinched,
                COUNT(*) FILTER (WHERE status = 'blocked') AS times_blocked
           FROM usage_events
          WHERE observed_at > now() - make_interval(days => $1::int)
          GROUP BY limit_type
          ORDER BY times_blocked DESC, limit_type",
    )
    .bind(days as i32)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Pattern {
            limit_type: r.get("limit_type"),
            days_seen: r.get("days_seen"),
            days_pinched: r.get("days_pinched"),
            times_blocked: r.get("times_blocked"),
        })
        .collect())
}

/// A limit whose reset has passed is telling you about a window that has
/// already refilled, so it is not news any more.
pub fn is_current(limit: &Limit, now: DateTime<Utc>) -> bool {
    limit.resets_at.is_none_or(|r| r > now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn limit(resets: Option<i64>) -> Limit {
        Limit {
            engine: "claude-code".into(),
            limit_type: "seven_day".into(),
            status: "warning".into(),
            resets_at: resets.map(at),
            using_overage: false,
            updated_at: at(1_000),
        }
    }

    #[test]
    fn the_same_news_twice_is_not_recorded_twice() {
        // A run prints this continuously while it works. Every one of those
        // saying "allowed, resets Thursday" is the same fact, and a row each
        // would be thousands a day that nobody can read.
        let same = Some(("allowed", Some(at(9_000))));
        assert!(!is_transition(same, "allowed", Some(at(9_000))));

        // A change of status is the point of the log.
        assert!(is_transition(same, "warning", Some(at(9_000))));
        assert!(is_transition(
            Some(("warning", Some(at(9_000)))),
            "blocked",
            Some(at(9_000))
        ));

        // And so is a new window at the *same* status — "allowed again, resets
        // next Thursday" is the week turning over, which is the most useful
        // line in the history and invisible if only the status is compared.
        assert!(is_transition(same, "allowed", Some(at(600_000))));

        // Nothing heard from this limit before: worth the first row.
        assert!(is_transition(None, "allowed", None));
    }

    #[test]
    fn a_window_that_has_already_refilled_is_not_news() {
        // The stale case: a warning from last Tuesday still sitting in the
        // table would otherwise show as though the user were near the edge now.
        assert!(!is_current(&limit(Some(500)), at(1_000)));
        assert!(is_current(&limit(Some(2_000)), at(1_000)));
        // No reset time at all: nothing says it has expired, so it stands.
        assert!(is_current(&limit(None), at(1_000)));
    }
}
