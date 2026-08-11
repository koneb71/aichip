//! Where the tokens went.
//!
//! The daily cap answers "may I start another one"; this answers "why is that
//! number what it is". They are different questions and only the second one
//! tells you what to change.
//!
//! Everything here is derived from figures the engines themselves reported —
//! no price table of aichip's own. That matters for the gaps: a run whose
//! engine never reported a cost has `cost_usd IS NULL`, and the honest
//! rendering of that is "unknown", never tokens multiplied by a rate we made
//! up. [`Totals::unpriced_runs`] exists so the gap is stated rather than
//! quietly rolled into a total that looks complete.

use crate::db::Db;
use sqlx::Row;
use uuid::Uuid;

/// The project a run belongs to is reachable by four different routes
/// depending on what kind of run it is. Same join the activity view uses; kept
/// identical on purpose, so the two screens can never disagree about which
/// runs belong to a workspace.
const PROJECT_JOIN: &str = "
    LEFT JOIN tasks     t ON t.id = r.task_id
    LEFT JOIN workflows w ON w.id = r.workflow_id
    LEFT JOIN chats     c ON c.id = r.chat_id
    LEFT JOIN projects  p ON p.id = COALESCE(r.project_id, t.project_id, w.project_id, c.project_id)";

/// Which aichip feature produced this run.
///
/// Derived, never stored. Every one of these is already distinguishable from
/// columns the orchestrator itself switches on to dispatch the run, so a
/// stored copy could only ever drift out of agreement with the real thing.
///
/// Order is most-specific-first and load-bearing: a board task handed to a
/// team has both `task_id` and `team_id`, and it is the team that explains
/// the cost.
const PATTERN_CASE: &str = "
    CASE WHEN r.variant_label  IS NOT NULL THEN 'bakeoff'
         WHEN r.kb_article_id  IS NOT NULL THEN 'knowledge'
         WHEN r.comment_id     IS NOT NULL THEN 'mention'
         WHEN r.team_id        IS NOT NULL THEN 'team'
         WHEN r.chat_id        IS NOT NULL THEN 'chat'
         WHEN r.workflow_id    IS NOT NULL THEN 'workflow'
         WHEN r.task_id        IS NOT NULL THEN 'task'
         ELSE 'other' END";

/// Token and cost sums for a window.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub cost_usd: f64,
    pub runs: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    /// Runs whose counters include mid-run estimates nothing reconciled.
    pub provisional_runs: i64,
    /// Runs that spent tokens but whose engine never reported a price. Their
    /// tokens are in the totals above; their cost is in nobody's.
    pub unpriced_runs: i64,
}

/// One row of a breakdown — by project, tier, engine, pattern, whatever.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Slice {
    pub key: String,
    pub cost_usd: f64,
    pub runs: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    /// Median rather than mean: one runaway run shouldn't set the expectation
    /// for the next ten. Same reasoning as the team launch estimate.
    pub median_usd: Option<f64>,
}

/// A day on the trend line.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayPoint {
    pub day: chrono::DateTime<chrono::Utc>,
    pub cost_usd: f64,
    pub runs: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
}

/// Which dimension to slice spend by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Project,
    Engine,
    Model,
    Tier,
    Pattern,
}

impl Dimension {
    /// The SQL expression this dimension groups on.
    fn sql(self) -> &'static str {
        match self {
            Self::Project => "COALESCE(p.name, 'unknown')",
            Self::Engine => "r.engine",
            Self::Model => "COALESCE(r.model, 'unknown')",
            // The tier actually used, not the one the card asked for.
            Self::Tier => "COALESCE(r.tier_override, t.model_tier, 'unknown')",
            Self::Pattern => PATTERN_CASE,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "project" => Some(Self::Project),
            "engine" => Some(Self::Engine),
            "model" => Some(Self::Model),
            "tier" => Some(Self::Tier),
            "pattern" => Some(Self::Pattern),
            _ => None,
        }
    }
}

/// The same sums, for one project.
///
/// Built from the same `PROJECT_JOIN` as `totals` rather than a second join of
/// its own — that const's doc says it is "kept identical on purpose, so the two
/// screens can never disagree about which runs belong to a workspace", and a
/// third reader must not be the one that breaks it.
pub async fn for_project(db: &Db, project_id: Uuid, days: i32) -> anyhow::Result<Totals> {
    let sql = format!(
        "SELECT COALESCE(SUM(r.cost_usd), 0)              AS cost,
                COUNT(*)                                   AS runs,
                COALESCE(SUM(r.input_tokens), 0)::bigint           AS input,
                COALESCE(SUM(r.output_tokens), 0)::bigint          AS output,
                COALESCE(SUM(r.cache_read_tokens), 0)::bigint      AS cache_read,
                COALESCE(SUM(r.cache_creation_tokens), 0)::bigint  AS cache_creation,
                COUNT(*) FILTER (WHERE r.tokens_provisional) AS provisional,
                COUNT(*) FILTER (WHERE r.cost_usd IS NULL AND r.input_tokens > 0) AS unpriced
         FROM runs r {PROJECT_JOIN}
         WHERE r.created_at > now() - make_interval(days => $2)
           AND p.id = $1"
    );
    let row = sqlx::query(&sql)
        .bind(project_id)
        .bind(days)
        .fetch_one(&db.pool)
        .await?;
    Ok(Totals {
        cost_usd: row.get("cost"),
        runs: row.get("runs"),
        input_tokens: row.get("input"),
        output_tokens: row.get("output"),
        cache_read_tokens: row.get("cache_read"),
        cache_creation_tokens: row.get("cache_creation"),
        provisional_runs: row.get("provisional"),
        unpriced_runs: row.get("unpriced"),
    })
}

/// Sums across the whole window.
pub async fn totals(db: &Db, ws: Option<Uuid>, days: i32) -> anyhow::Result<Totals> {
    let sql = format!(
        "SELECT COALESCE(SUM(r.cost_usd), 0)              AS cost,
                COUNT(*)                                   AS runs,
                COALESCE(SUM(r.input_tokens), 0)::bigint           AS input,
                COALESCE(SUM(r.output_tokens), 0)::bigint          AS output,
                COALESCE(SUM(r.cache_read_tokens), 0)::bigint      AS cache_read,
                COALESCE(SUM(r.cache_creation_tokens), 0)::bigint  AS cache_creation,
                COUNT(*) FILTER (WHERE r.tokens_provisional) AS provisional,
                COUNT(*) FILTER (WHERE r.cost_usd IS NULL AND r.input_tokens > 0) AS unpriced
         FROM runs r {PROJECT_JOIN}
         WHERE r.created_at > now() - make_interval(days => $2)
           AND ($1::uuid IS NULL OR p.workspace_id = $1)"
    );
    let row = sqlx::query(&sql)
        .bind(ws)
        .bind(days)
        .fetch_one(&db.pool)
        .await?;
    Ok(Totals {
        cost_usd: row.get("cost"),
        runs: row.get("runs"),
        input_tokens: row.get("input"),
        output_tokens: row.get("output"),
        cache_read_tokens: row.get("cache_read"),
        cache_creation_tokens: row.get("cache_creation"),
        provisional_runs: row.get("provisional"),
        unpriced_runs: row.get("unpriced"),
    })
}

/// Spend broken down one way, dearest first.
pub async fn by(
    db: &Db,
    ws: Option<Uuid>,
    days: i32,
    dim: Dimension,
) -> anyhow::Result<Vec<Slice>> {
    let sql = format!(
        "SELECT {} AS key,
                COALESCE(SUM(r.cost_usd), 0)              AS cost,
                COUNT(*)                                   AS runs,
                COALESCE(SUM(r.input_tokens), 0)::bigint           AS input,
                COALESCE(SUM(r.output_tokens), 0)::bigint          AS output,
                COALESCE(SUM(r.cache_read_tokens), 0)::bigint      AS cache_read,
                COALESCE(SUM(r.cache_creation_tokens), 0)::bigint  AS cache_creation,
                percentile_cont(0.5) WITHIN GROUP (ORDER BY r.cost_usd) AS median
         FROM runs r {PROJECT_JOIN}
         WHERE r.created_at > now() - make_interval(days => $2)
           AND ($1::uuid IS NULL OR p.workspace_id = $1)
         GROUP BY 1 ORDER BY cost DESC NULLS LAST LIMIT 20",
        dim.sql()
    );
    let rows = sqlx::query(&sql)
        .bind(ws)
        .bind(days)
        .fetch_all(&db.pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| Slice {
            key: r.get("key"),
            cost_usd: r.get("cost"),
            runs: r.get("runs"),
            input_tokens: r.get("input"),
            output_tokens: r.get("output"),
            cache_read_tokens: r.get("cache_read"),
            cache_creation_tokens: r.get("cache_creation"),
            median_usd: r.get("median"),
        })
        .collect())
}

/// The trend line. Quiet days are absent here and filled in by the caller —
/// a day with no runs has no row to return.
pub async fn by_day(db: &Db, ws: Option<Uuid>, days: i32) -> anyhow::Result<Vec<DayPoint>> {
    let sql = format!(
        "SELECT date_trunc('day', r.created_at)          AS day,
                COALESCE(SUM(r.cost_usd), 0)              AS cost,
                COUNT(*)                                   AS runs,
                COALESCE(SUM(r.input_tokens), 0)::bigint           AS input,
                COALESCE(SUM(r.output_tokens), 0)::bigint          AS output,
                COALESCE(SUM(r.cache_read_tokens), 0)::bigint      AS cache_read
         FROM runs r {PROJECT_JOIN}
         WHERE r.created_at > now() - make_interval(days => $2)
           AND ($1::uuid IS NULL OR p.workspace_id = $1)
         GROUP BY 1 ORDER BY 1"
    );
    let rows = sqlx::query(&sql)
        .bind(ws)
        .bind(days)
        .fetch_all(&db.pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| DayPoint {
            day: r.get("day"),
            cost_usd: r.get("cost"),
            runs: r.get("runs"),
            input_tokens: r.get("input"),
            output_tokens: r.get("output"),
            cache_read_tokens: r.get("cache_read"),
        })
        .collect())
}

/// What fraction of everything sent was served from cache.
///
/// The headline number, because it is the one a person can act on: fresh input
/// is the expensive kind, and a run that reuses its context pays a fraction of
/// what a run that rebuilds it does.
///
/// `None` when nothing was sent at all — a rate of zero would claim the cache
/// missed, and "no requests yet" is a different fact from "every request
/// missed".
pub fn cache_hit_rate(input: i64, cache_read: i64, cache_creation: i64) -> Option<f64> {
    let sent = input.saturating_add(cache_read).saturating_add(cache_creation);
    if sent <= 0 {
        return None;
    }
    Some(cache_read as f64 / sent as f64)
}

/// How many times the baseline one thing costs relative to another.
///
/// Used to say "a bake-off here has cost 2.6× a plain run". `None` when the
/// baseline is zero or missing, because everything is infinitely more
/// expensive than nothing and saying so helps no one.
pub fn multiple_of_baseline(value: Option<f64>, baseline: Option<f64>) -> Option<f64> {
    match (value, baseline) {
        (Some(v), Some(b)) if b > 0.0 => Some(v / b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_rate_is_a_share_of_everything_sent() {
        // 12800 of 13860 tokens came from cache — the shape of a cheap run.
        let r = cache_hit_rate(260, 12_800, 800).unwrap();
        assert!((r - 0.9235).abs() < 0.001, "got {r}");
    }

    #[test]
    fn no_requests_is_not_a_zero_percent_hit_rate() {
        // A brand-new install must not read as "your cache never works".
        assert_eq!(cache_hit_rate(0, 0, 0), None);
    }

    #[test]
    fn a_run_that_cached_nothing_is_zero_not_none() {
        assert_eq!(cache_hit_rate(500, 0, 0), Some(0.0));
    }

    #[test]
    fn cache_hit_rate_does_not_overflow_on_absurd_totals() {
        // Saturating rather than panicking in a release-mode arithmetic edge.
        assert!(cache_hit_rate(i64::MAX, i64::MAX, i64::MAX).is_some());
    }

    #[test]
    fn a_multiple_needs_a_real_baseline() {
        assert_eq!(multiple_of_baseline(Some(2.6), Some(1.0)), Some(2.6));
        // Dividing by a project with no plain runs yet would be infinity.
        assert_eq!(multiple_of_baseline(Some(2.6), Some(0.0)), None);
        assert_eq!(multiple_of_baseline(Some(2.6), None), None);
        assert_eq!(multiple_of_baseline(None, Some(1.0)), None);
    }

    #[test]
    fn every_dimension_round_trips_through_its_name() {
        // The API takes these as strings; a name that doesn't parse back is a
        // dimension the dashboard can ask for and never receive.
        for (name, dim) in [
            ("project", Dimension::Project),
            ("engine", Dimension::Engine),
            ("model", Dimension::Model),
            ("tier", Dimension::Tier),
            ("pattern", Dimension::Pattern),
        ] {
            assert_eq!(Dimension::parse(name), Some(dim));
            assert!(!dim.sql().is_empty());
        }
        assert_eq!(Dimension::parse("nonsense"), None);
    }
}
