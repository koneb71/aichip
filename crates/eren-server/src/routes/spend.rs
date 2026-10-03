//! Where the tokens went.
//!
//! Deliberately **not** part of `/api/activity`. That endpoint is polled every
//! few seconds to answer "what is running right now"; six grouped aggregates
//! over the run history is not a question worth asking at that rate. This one
//! is fetched when someone opens the page.

use super::{internal, ApiError};
use crate::auth::Caller;
use crate::AppState;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use eren_core::spend::{self, DayPoint, Dimension, Slice, Totals};
use eren_core::Db;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/spend", get(overview))
}

#[derive(Deserialize)]
struct Window {
    workspace_id: Option<Uuid>,
    days: Option<i32>,
}

impl Window {
    /// A window has to be bounded — an unbounded one scans the whole history
    /// and gets slower every week the install is used. 30 days by default,
    /// a year at most.
    fn days(&self) -> i32 {
        self.days.unwrap_or(30).clamp(1, 365)
    }
}

async fn overview(
    State(state): State<AppState>,
    caller: Caller,
    Query(q): Query<Window>,
) -> Result<Json<Value>, ApiError> {
    let ws = caller.workspace_filter(&state, q.workspace_id).await?;
    let ws = ws.as_deref();
    let days = q.days();
    let db = &state.db;

    let totals = totals(db, ws, days).await.map_err(internal)?;
    let by_day = by_day(db, ws, days).await.map_err(internal)?;

    let mut breakdowns = serde_json::Map::new();
    for (name, dim) in [
        ("project", Dimension::Project),
        ("engine", Dimension::Engine),
        ("model", Dimension::Model),
        ("tier", Dimension::Tier),
        ("pattern", Dimension::Pattern),
        ("agent", Dimension::Agent),
        ("routine", Dimension::Routine),
    ] {
        let slices = by(db, ws, days, dim).await.map_err(internal)?;
        breakdowns.insert(name.to_string(), serde_json::to_value(slices).unwrap());
    }

    // Computed here rather than in SQL so the divide-by-zero case is decided
    // in one tested place: "nothing sent yet" and "every request missed" are
    // different facts and a bare 0.0 would conflate them.
    let hit_rate = spend::cache_hit_rate(
        totals.input_tokens,
        totals.cache_read_tokens,
        totals.cache_creation_tokens,
    );

    Ok(Json(json!({
        "days": days,
        "totals": totals,
        "cacheHitRate": hit_rate,
        "byDay": by_day,
        "breakdowns": breakdowns,
    })))
}

// ── Scoped to a set of workspaces ───────────────────────────────────────────
//
// `eren_core::spend` answers for one workspace or for all of them, and a
// signed-in person who asks for no workspace in particular means *their*
// workspaces — never all. So each one is asked on its own and the answers
// are added up here. `None` is every workspace (accounts off), one call as
// before; an empty set is someone with no workspace yet, who has spent
// nothing.

/// The workspaces to ask one at a time: `[None]` for every workspace.
fn each(ws: Option<&[Uuid]>) -> Vec<Option<Uuid>> {
    match ws {
        None => vec![None],
        Some(ids) => ids.iter().copied().map(Some).collect(),
    }
}

async fn totals(db: &Db, ws: Option<&[Uuid]>, days: i32) -> anyhow::Result<Totals> {
    let mut sum = Totals::default();
    for w in each(ws) {
        let t = spend::totals(db, w, days).await?;
        sum.cost_usd += t.cost_usd;
        sum.runs += t.runs;
        sum.input_tokens += t.input_tokens;
        sum.output_tokens += t.output_tokens;
        sum.cache_read_tokens += t.cache_read_tokens;
        sum.cache_creation_tokens += t.cache_creation_tokens;
        sum.provisional_runs += t.provisional_runs;
        sum.unpriced_runs += t.unpriced_runs;
    }
    Ok(sum)
}

async fn by_day(db: &Db, ws: Option<&[Uuid]>, days: i32) -> anyhow::Result<Vec<DayPoint>> {
    let mut out: Vec<DayPoint> = vec![];
    for w in each(ws) {
        for p in spend::by_day(db, w, days).await? {
            match out.iter_mut().find(|d| d.day == p.day) {
                Some(d) => {
                    d.cost_usd += p.cost_usd;
                    d.runs += p.runs;
                    d.input_tokens += p.input_tokens;
                    d.output_tokens += p.output_tokens;
                    d.cache_read_tokens += p.cache_read_tokens;
                }
                None => out.push(p),
            }
        }
    }
    out.sort_by_key(|d| d.day);
    Ok(out)
}

/// A breakdown over several workspaces, dearest first. Slices with the same
/// key add up; their median cannot be added, so a key met in more than one
/// workspace says it has none rather than quoting one workspace's.
pub(super) async fn by(
    db: &Db,
    ws: Option<&[Uuid]>,
    days: i32,
    dim: Dimension,
) -> anyhow::Result<Vec<Slice>> {
    let asks = each(ws);
    if asks.len() == 1 {
        return spend::by(db, asks[0], days, dim).await;
    }
    let mut out: Vec<Slice> = vec![];
    for w in asks {
        for s in spend::by(db, w, days, dim).await? {
            match out.iter_mut().find(|o| o.key == s.key) {
                Some(o) => {
                    o.cost_usd += s.cost_usd;
                    o.runs += s.runs;
                    o.input_tokens += s.input_tokens;
                    o.output_tokens += s.output_tokens;
                    o.cache_read_tokens += s.cache_read_tokens;
                    o.cache_creation_tokens += s.cache_creation_tokens;
                    o.median_usd = None;
                }
                None => out.push(s),
            }
        }
    }
    out.sort_by(|a, b| b.cost_usd.total_cmp(&a.cost_usd));
    // The cap `spend::by` keeps for one workspace.
    out.truncate(20);
    Ok(out)
}
