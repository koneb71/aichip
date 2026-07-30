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
    Router::new().route("/github", get(status))
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
