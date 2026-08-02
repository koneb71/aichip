//! The gallery's own endpoints.
//!
//! Everything here is a person acting through the dashboard, so it all sits
//! behind the ordinary loopback checks like every other route. What an *app*
//! may ask for is a different surface with a different gate, and does not live
//! here.

use super::{internal, ApiError};
use crate::AppState;
use aichip_core::apps;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/apps", get(list).post(install))
        .route("/apps/{id}", get(one).delete(uninstall))
        .route("/apps/{id}/manifest", put(set_manifest))
        .route("/apps/{id}/active", post(set_active))
}

fn app_json(app: &apps::App) -> Value {
    json!({
        "id": app.id,
        "projectId": app.project_id,
        "workspaceId": app.workspace_id,
        "slug": app.slug,
        "name": app.name,
        "icon": app.icon,
        "summary": app.summary,
        "brief": app.brief,
        "runtime": app.runtime.as_str(),
        "active": app.active,
        "path": app.path.to_string_lossy(),
    })
}

/// A manifest problem is the person's to fix, not a server fault.
///
/// 400 with the parser's own text, which names the key — "models.expense.
/// fields.qty: unknown field type" is something to act on, where a 500 is not.
fn bad_manifest(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, e.to_string())
}

#[derive(Deserialize)]
pub struct WorkspaceFilter {
    pub workspace_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<WorkspaceFilter>,
) -> Result<Json<Value>, ApiError> {
    let apps = apps::list(&state.db, filter.workspace_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "apps": apps.iter().map(app_json).collect::<Vec<_>>() })))
}

async fn one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let app = load(&state, id).await?;
    let mut body = app_json(&app);
    body["manifest"] = json!(app.manifest);
    Ok(Json(body))
}

#[derive(Deserialize)]
struct Install {
    workspace_id: Uuid,
    manifest: String,
    #[serde(default)]
    brief: String,
}

async fn install(
    State(state): State<AppState>,
    Json(body): Json<Install>,
) -> Result<Json<Value>, ApiError> {
    // Validated here as well as inside `install`, so a manifest an agent got
    // wrong comes back as a 400 naming the key rather than a 500 naming
    // nothing. The check inside is what makes the guarantee; this is what makes
    // the message.
    apps::manifest::parse(&body.manifest).map_err(bad_manifest)?;
    let app = apps::install(&state.db, body.workspace_id, &body.manifest, &body.brief)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(app_json(&app)))
}

#[derive(Deserialize)]
struct SetManifest {
    manifest: String,
}

async fn set_manifest(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetManifest>,
) -> Result<Json<Value>, ApiError> {
    let app = load(&state, id).await?;
    apps::set_manifest(&state.db, &app, &body.manifest)
        .await
        .map_err(bad_manifest)?;
    let app = load(&state, id).await?;
    let mut out = app_json(&app);
    out["manifest"] = json!(app.manifest);
    Ok(Json(out))
}

#[derive(Deserialize)]
struct SetActive {
    active: bool,
}

async fn set_active(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetActive>,
) -> Result<Json<Value>, ApiError> {
    load(&state, id).await?;
    apps::set_active(&state.db, id, body.active)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "active": body.active })))
}

/// Remove an app, its tables and its folder.
///
/// The only destructive verb here. Deactivating is what "I am done with this
/// for now" means, and it keeps everything; this is the one that does not, so
/// the dashboard asks before calling it.
async fn uninstall(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    apps::uninstall(&state.db, id).await.map_err(internal)?;
    Ok(Json(json!({ "ok": true })))
}

async fn load(state: &AppState, id: Uuid) -> Result<apps::App, ApiError> {
    apps::get(&state.db, id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such app".into()))
}
