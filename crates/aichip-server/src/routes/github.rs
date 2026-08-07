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
        .route("/github/scopes", get(scopes))
        .route("/github/connect", axum::routing::post(connect))
        .route(
            "/github/connect/{id}",
            get(connect_status).delete(cancel_connect),
        )
        .route("/github/clone", axum::routing::post(clone))
        .route(
            "/github/clone/{id}",
            get(clone_status).delete(cancel_clone),
        )
}

/// Clone a repository into a new folder and make it a project.
///
/// Returns as soon as the clone has *started*. A repository of any size takes
/// longer than a request should, and `previews.rs` already says what happens to
/// a button that hangs for minutes: it gets pressed twice.
#[derive(serde::Deserialize)]
struct CloneRepo {
    workspace_id: uuid::Uuid,
    /// Anything a person pastes: `owner/repo`, an https URL, an ssh URL.
    repo: String,
    /// Where to put it. Defaults to the folder the browser opens in.
    parent: Option<String>,
    /// What to call the folder. Defaults to the repository's own name.
    name: Option<String>,
}

async fn clone(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<CloneRepo>,
) -> Result<Json<Value>, crate::routes::ApiError> {
    use aichip_core::github::repo;
    use axum::http::StatusCode;

    let parsed = repo::parse_repo_ref(&body.repo).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // The host must be one `gh` is actually signed in to. A real check against
    // what was already fetched, rather than a hardcoded allowlist that would
    // refuse an enterprise host somebody legitimately uses.
    let info = aichip_core::github::detect().await.ok_or((
        StatusCode::CONFLICT,
        "the GitHub CLI (gh) is not installed, so aichip cannot clone anything".to_string(),
    ))?;
    if !info.usable() {
        let problem = info
            .active()
            .and_then(|a| a.problem.clone())
            .unwrap_or_else(|| "no account is signed in".into());
        return Err((
            StatusCode::CONFLICT,
            format!("the GitHub CLI is installed but not usable: {problem}"),
        ));
    }
    let known = info.accounts.iter().any(|a| a.host == parsed.host);
    if !known && parsed.host != "github.com" {
        return Err((
            StatusCode::CONFLICT,
            format!("aichip is not signed in to {} — sign in from Connections first", parsed.host),
        ));
    }

    // The destination is validated as sandboxed parent plus checked leaf, which
    // is the only shape available: `sandboxed` canonicalizes, so it can say
    // nothing about a folder that does not exist yet. Same as `fs::mkdir`.
    let root = crate::routes::fs::browse_root();
    let parent = body.parent.map(std::path::PathBuf::from).unwrap_or_else(|| root.clone());
    let parent = crate::routes::fs::sandboxed(&root, &parent).ok_or((
        StatusCode::FORBIDDEN,
        "that path is outside the folder aichip is allowed to browse".to_string(),
    ))?;

    let default_name = repo::destination_name(&parsed).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let wanted = body.name.unwrap_or_else(|| default_name.to_string());
    let name = crate::routes::fs::safe_dir_name(&wanted)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?
        .to_string();

    let (id, destination) = repo::start_clone(&parsed, &parent, &name, body.workspace_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let _ = &state;
    Ok(Json(json!({ "id": id, "destination": destination.to_string_lossy() })))
}

async fn clone_status(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> Json<Value> {
    let progress = aichip_core::github::repo::poll_clone(&state.db, id).await;
    Json(serde_json::to_value(progress).unwrap_or(Value::Null))
}

async fn cancel_clone(axum::extract::Path(id): axum::extract::Path<uuid::Uuid>) -> Json<Value> {
    aichip_core::github::repo::cancel_clone(id).await;
    Json(json!({ "cancelled": true }))
}

/// What signing in will ask for, so it can be said before the button is pressed.
///
/// The required set is `gh`'s, not ours — it refuses to go below `repo`,
/// `read:org` and `gist`. Stating that is more useful than a switch that
/// pretends otherwise.
async fn scopes() -> Json<Value> {
    use aichip_core::github::connect::{OPTIONAL_SCOPES, REQUIRED_SCOPES};
    Json(json!({
        "required": REQUIRED_SCOPES,
        "optional": OPTIONAL_SCOPES
            .iter()
            .map(|(name, what)| json!({ "name": name, "what": what }))
            .collect::<Vec<_>>(),
    }))
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
#[derive(serde::Deserialize, Default)]
struct ConnectBody {
    /// Beyond what `gh` already requires. Absent means "nothing extra", which
    /// is the default and the right one — organisation access is granted per
    /// organisation on GitHub's own page, not widened from here.
    #[serde(default)]
    scopes: Vec<String>,
}

async fn connect(
    body: Option<Json<ConnectBody>>,
) -> Result<Json<Value>, super::ApiError> {
    let scopes = body.map(|Json(b)| b.scopes).unwrap_or_default();
    let started = aichip_core::github::connect::start(&scopes)
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
