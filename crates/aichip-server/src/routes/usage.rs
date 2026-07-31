//! Where the user's plan stands.
//!
//! No credential and no call to Anthropic: this is what their own CLI printed
//! while it worked, kept rather than discarded. So it is only as fresh as the
//! last run — which is stated, because a figure with no age on it invites being
//! read as live.

use crate::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new().route("/usage", get(current))
}

async fn current(State(state): State<AppState>) -> Json<Value> {
    let limits = aichip_core::usage::all(&state.db).await.unwrap_or_default();
    let now = chrono::Utc::now();
    // A window that has already refilled says nothing about now.
    let limits: Vec<_> = limits
        .into_iter()
        .filter(|l| aichip_core::usage::is_current(l, now))
        .collect();
    let worst = limits
        .iter()
        .find(|l| l.status != "allowed")
        .map(|l| l.status.clone());
    Json(json!({
        "limits": limits,
        // The one thing a header chip needs: is anything worth mentioning.
        "worst": worst,
    }))
}
