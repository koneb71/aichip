//! Whether this machine can talk to GitHub, and as whom.
//!
//! Same reasoning as `/api/engines`: the UI asks what actually exists rather
//! than assuming, so a GitHub action is never offered to someone whose `gh` is
//! missing or whose token has expired. Finding that out at the point of the
//! click is much cheaper than finding it out from a failed push.
//!
//! Probed live rather than cached at boot, because `gh auth login` happens in
//! another terminal while aichip is running — and the whole reason to show this
//! is to tell someone to go and do exactly that.

use crate::AppState;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/github", get(status))
        .route("/github/connect", axum::routing::post(connect))
        .route(
            "/github/connect/{id}",
            get(connect_status).delete(cancel_connect),
        )
}

/// Begin GitHub's device flow and hand back the code to show.
///
/// aichip never sees the token this produces. `gh` runs the flow, GitHub gives
/// the credential straight to `gh`, and `gh` stores it. What comes back here is
/// a one-time code whose entire purpose is to be shown to the person.
///
/// A field to paste a personal access token into would have been fewer moving
/// parts and is deliberately not what this is: aichip does not receive, carry
/// or store credentials for any provider.
async fn connect() -> Result<Json<Value>, super::ApiError> {
    let started = aichip_core::github::connect::start()
        .await
        .map_err(|e| (axum::http::StatusCode::BAD_GATEWAY, e.to_string()))?;
    Ok(Json(serde_json::to_value(started).unwrap_or(Value::Null)))
}

async fn connect_status(
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Json<Value> {
    let progress = aichip_core::github::connect::poll(id).await;
    Json(serde_json::to_value(progress).unwrap_or(Value::Null))
}

async fn cancel_connect(
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Json<Value> {
    aichip_core::github::connect::cancel(id).await;
    Json(json!({ "cancelled": true }))
}

async fn status() -> Json<Value> {
    let Some(info) = aichip_core::github::detect().await else {
        return Json(json!({
            "installed": false,
            "usable": false,
            "accounts": [],
        }));
    };
    Json(json!({
        "installed": true,
        "usable": info.usable(),
        "version": info.version,
        // Host and login only. `gh` also reports where it keeps its token;
        // aichip does not ask for that and would not pass it on if it did.
        "accounts": info.accounts.iter().map(|a| json!({
            "host": a.host,
            "login": a.login,
            "active": a.active,
            "valid": a.valid,
            "problem": a.problem,
        })).collect::<Vec<_>>(),
    }))
}
