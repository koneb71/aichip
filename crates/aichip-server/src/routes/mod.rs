pub mod agents;
pub mod attachments;
pub mod chat;
pub mod files;
pub mod fs;
pub mod orgs;
pub mod projects;
pub mod search;
pub mod tasks;
pub mod teams;
pub mod workflows;
pub mod workspaces;

use crate::AppState;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

pub type ApiError = (StatusCode, String);

pub fn internal(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .merge(workspaces::router())
        .merge(projects::router())
        .merge(tasks::router())
        .merge(agents::router())
        .merge(teams::router())
        .merge(orgs::router())
        .merge(workflows::router())
        .merge(fs::router())
        .merge(files::router())
        .merge(attachments::router())
        .merge(search::router())
        .merge(chat::router())
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "name": "aichip" }))
}
