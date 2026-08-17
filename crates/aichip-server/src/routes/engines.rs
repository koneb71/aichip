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
    Router::new()
        .route("/engines", get(list))
        // Beside the engines and deliberately not one of them: see
        // `aichip_core::local_models`. Ollama and LM Studio are providers
        // OpenCode fronts, not agents aichip can drive.
        .route("/local-models", get(local_models))
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

/// Models the local runtimes on this machine have pulled.
///
/// Answers with an empty list rather than an error when nothing is running,
/// which is the common case — a settings page must not look broken because a
/// thing the user never installed is not listening.
async fn local_models() -> Json<Value> {
    Json(json!({ "models": aichip_core::local_models::discover().await }))
}
