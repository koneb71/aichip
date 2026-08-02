//! Where the tokens went.
//!
//! Deliberately **not** part of `/api/activity`. That endpoint is polled every
//! few seconds to answer "what is running right now"; six grouped aggregates
//! over the run history is not a question worth asking at that rate. This one
//! is fetched when someone opens the page.

use super::{internal, ApiError};
use crate::AppState;
use aichip_core::spend::{self, Dimension};
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
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
    Query(q): Query<Window>,
) -> Result<Json<Value>, ApiError> {
    let ws = q.workspace_id;
    let days = q.days();
    let db = &state.db;

    let totals = spend::totals(db, ws, days).await.map_err(internal)?;
    let by_day = spend::by_day(db, ws, days).await.map_err(internal)?;

    let mut breakdowns = serde_json::Map::new();
    for (name, dim) in [
        ("project", Dimension::Project),
        ("engine", Dimension::Engine),
        ("model", Dimension::Model),
        ("tier", Dimension::Tier),
        ("pattern", Dimension::Pattern),
    ] {
        let slices = spend::by(db, ws, days, dim).await.map_err(internal)?;
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
