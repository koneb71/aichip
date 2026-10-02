pub mod activity;
pub mod agents;
pub mod apps;
pub mod attachments;
pub mod audit;
pub mod budgets;
pub mod chat;
pub mod checks;
pub mod engines;
pub mod files;
pub mod fs;
pub mod github;
pub mod inbox;
pub mod kb;
pub mod manager;
pub mod mcp_servers;
pub mod orgs;
pub mod previews;
pub mod projects;
pub mod pull_requests;
pub mod repo_map;
pub mod research;
pub mod reviews;
pub mod revisions;
pub mod routines;
pub mod search;
pub mod settings;
pub mod skills;
pub mod spaces;
pub mod spend;
pub mod tasks;
pub mod teams;
pub mod terminal;
pub mod usage;
pub mod workflows;
pub mod workspaces;

use crate::AppState;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

pub type ApiError = (StatusCode, String);

/// A run a door refused for a reason the person can act on — the card is
/// already running, a follow-up has nothing to act on, the agent is paused —
/// is a 409 that says so. Anything else is a fault.
pub fn run_refused(e: anyhow::Error) -> ApiError {
    if e.is::<aichip_core::runs::orchestrator::AlreadyRunning>()
        || e.is::<aichip_core::runs::follow_up::FollowUpRefusal>()
        || e.is::<aichip_core::agents::Unavailable>()
        || e.is::<aichip_core::budgets::OverBudget>()
    {
        (axum::http::StatusCode::CONFLICT, e.to_string())
    } else {
        internal(e)
    }
}

/// [`run_refused`] for a door whose other failures are the caller's fault
/// (a 400) rather than a fault: a refusal still reads as a 409.
pub fn refused_or(status: StatusCode) -> impl Fn(anyhow::Error) -> ApiError {
    move |e| match run_refused(e) {
        (StatusCode::INTERNAL_SERVER_ERROR, message) => (status, message),
        refused => refused,
    }
}

/// The header a dashboard write carries. Its only job is to be un-settable by
/// a cross-origin simple request: there is no CORS layer, so a preflight for
/// it gets no `Access-Control-Allow-*` and the browser refuses to send the
/// real request. The value is not a secret and is checked against nothing.
/// Belt and braces behind the Origin check in `lib.rs`.
pub const WRITE_HEADER: &str = "x-aichip-write";

/// Refuse a write that did not come from the dashboard. `what` says what the
/// endpoint does, so the refusal explains why it is gated.
pub fn require_write(headers: &axum::http::HeaderMap, what: &str) -> Result<(), ApiError> {
    if headers.contains_key(WRITE_HEADER) {
        Ok(())
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            format!("{what}, so it needs the {WRITE_HEADER} header"),
        ))
    }
}

/// An answer to something waiting on a person, refused — said in HTTP.
pub fn answer_refused(e: aichip_core::approvals::Refusal) -> ApiError {
    use aichip_core::approvals::Refusal;
    match e {
        Refusal::NotFound(m) => (StatusCode::NOT_FOUND, m),
        Refusal::Conflict(m) => (StatusCode::CONFLICT, m),
        Refusal::Invalid(m) => (StatusCode::BAD_REQUEST, m),
        Refusal::Gated(e) => run_refused(e),
        Refusal::Internal(e) => internal(e),
    }
}

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
        .merge(checks::router())
        .merge(reviews::router())
        .merge(budgets::router())
        .merge(inbox::router())
        .merge(audit::router())
        .merge(revisions::router())
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
        .merge(repo_map::router())
        .merge(research::router())
        .merge(manager::router())
        .merge(routines::router())
        .merge(spaces::router())
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "name": "aichip" }))
}
