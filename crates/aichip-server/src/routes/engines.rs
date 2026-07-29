//! Which agent CLIs this machine actually has.
//!
//! Every engine picker in the UI is driven from here rather than from a list
//! baked into the bundle. That is the whole point: offering "OpenCode" to
//! someone who hasn't installed it produces a run that fails at spawn time,
//! long after the choice was made and nowhere near it.
//!
//! Capabilities travel with each engine so the UI can *disable* an option and
//! say why, instead of letting the server refuse it after the click.

use crate::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new().route("/engines", get(list))
}

async fn list(State(state): State<AppState>) -> Json<Value> {
    let engines = state
        .orchestrator
        .engines()
        .into_iter()
        // The mock engine is a test fixture, not something to offer a user.
        .filter(|e| e.id() != "mock")
        .map(|e| {
            let info = state.orchestrator.engine_info(e.id());
            json!({
                "id": e.id(),
                "label": e.label(),
                "version": info.map(|i| i.version.clone()),
                "authenticated": info.map(|i| i.authenticated).unwrap_or(true),
                // Names and auth *type* only — never a credential. Populated by
                // running the CLI, never by reading its config files.
                "providers": info.map(|i| i.providers.clone()).unwrap_or_default(),
                "capabilities": e.capabilities(),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({ "engines": engines }))
}
