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

/// Record what a run just heard. Called for every `rate_limit_event`.
pub async fn record(
    db: &Db,
    engine: &str,
    limit_type: &str,
    status: &str,
    resets_at: Option<DateTime<Utc>>,
    using_overage: bool,
) -> anyhow::Result<()> {
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
    fn a_window_that_has_already_refilled_is_not_news() {
        // The stale case: a warning from last Tuesday still sitting in the
        // table would otherwise show as though the user were near the edge now.
        assert!(!is_current(&limit(Some(500)), at(1_000)));
        assert!(is_current(&limit(Some(2_000)), at(1_000)));
        // No reset time at all: nothing says it has expired, so it stands.
        assert!(is_current(&limit(None), at(1_000)));
    }
}
