pub mod activity;
pub mod agents;
pub mod apps;
pub mod attachments;
pub mod chat;
pub mod engines;
pub mod kb;
pub mod files;
pub mod fs;
pub mod github;
pub mod previews;
pub mod pull_requests;
pub mod research;
pub mod routines;
pub mod usage;
pub mod mcp_servers;
pub mod orgs;
pub mod projects;
pub mod search;
pub mod settings;
pub mod skills;
pub mod spaces;
pub mod spend;
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
        .merge(apps::router())
        .merge(tasks::router())
        .merge(agents::router())
        .merge(skills::router())
        .merge(teams::router())
        .merge(orgs::router())
        .merge(workflows::router())
        .merge(fs::router())
        .merge(files::router())
        .merge(attachments::router())
        .merge(search::router())
        .merge(chat::router())
        .merge(activity::router())
        .merge(mcp_servers::router())
        .merge(settings::router())
        .merge(engines::router())
        .merge(github::router())
        .merge(previews::router())
        .merge(pull_requests::router())
        .merge(usage::router())
        .merge(spend::router())
        .merge(kb::router())
        .merge(research::router())
        .merge(routines::router())
        .merge(spaces::router())
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "name": "aichip" }))
}
