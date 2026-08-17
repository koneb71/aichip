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
        .route("/local-models", get(local_models).put(set_local_hosts))
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
async fn local_models(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "models": aichip_core::local_models::discover(&state.db).await,
        // The addresses it looked at, so a page showing nothing can say where
        // it looked rather than leaving somebody to guess.
        "hosts": aichip_core::local_models::hosts(&state.db).await,
    }))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostsBody {
    ollama: Option<String>,
    lmstudio: Option<String>,
}

/// Point the probe somewhere else. An empty string resets to the default.
async fn set_local_hosts(
    State(state): State<AppState>,
    Json(body): Json<HostsBody>,
) -> Result<Json<Value>, super::ApiError> {
    aichip_core::local_models::set_hosts(
        &state.db,
        body.ollama.as_deref(),
        body.lmstudio.as_deref(),
    )
    .await
    .map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({
        "hosts": aichip_core::local_models::hosts(&state.db).await,
        "models": aichip_core::local_models::discover(&state.db).await,
    })))
}
