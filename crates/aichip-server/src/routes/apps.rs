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
        .route("/apps/generate", post(generate))
        .route("/apps/{id}", get(one).delete(uninstall))
        .route("/apps/{id}/manifest", put(set_manifest))
        .route("/apps/{id}/active", post(set_active))
        .route("/apps/{id}/schema", get(schema_plan))
        .route("/apps/{id}/schema/apply", post(apply_schema))
        .route("/apps/{id}/schema/discard", post(discard_schema))
        .route("/apps/{id}/data/{model}", get(rows).post(add_row))
        .route(
            "/apps/{id}/data/{model}/{row}",
            get(row).patch(change_row).delete(drop_row),
        )
        .route("/apps/{id}/chart/{view}", get(chart))
}

fn plan_json(plan: &apps::PendingPlan) -> Value {
    json!({
        "id": plan.id,
        "statements": plan.statements.iter().map(|s| json!({
            "sql": s.sql,
            "destructive": s.destructive,
            "why": s.why,
        })).collect::<Vec<_>>(),
    })
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

/// One app, with everything a screen needs to draw it.
///
/// The parsed manifest travels alongside the text, so the browser never has to
/// read YAML and cannot end up with a different idea of what the app declares
/// than the server has. A manifest that no longer parses still returns the app
/// — with the error — because the way out of a broken manifest is the editor,
/// and a 500 would take that away.
async fn one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let app = load(&state, id).await?;
    let mut body = app_json(&app);
    body["manifest"] = json!(app.manifest);
    match app.parsed() {
        Ok(parsed) => body["declares"] = apps::render::manifest_json(&parsed),
        Err(e) => body["manifestError"] = json!(e.to_string()),
    }
    body["pending"] = apps::pending_plan(&state.db, id)
        .await
        .map_err(internal)?
        .as_ref()
        .map_or(Value::Null, plan_json);
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
struct Generate {
    description: String,
    engine: Option<String>,
}

/// Write a manifest from a description, and hand it back unsaved.
///
/// Returned rather than installed, deliberately. The whole reason a module is
/// YAML instead of code is that a person can read it before it becomes real —
/// installing it for them would spend that and give nothing back. Same shape as
/// `/api/agents/generate`.
///
/// The result is parsed here before it is returned, so a manifest the model got
/// wrong arrives with the error attached and the text still editable, rather
/// than looking fine until the install button fails.
async fn generate(
    State(state): State<AppState>,
    Json(body): Json<Generate>,
) -> Result<Json<Value>, ApiError> {
    let description = body.description.trim();
    if description.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "say what the app is for".into()));
    }

    let default_engine = state.orchestrator.default_engine();
    let engine_id = body.engine.as_deref().unwrap_or(&default_engine);
    let engine = state
        .orchestrator
        .engine(engine_id)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("unknown engine {engine_id}")))?;
    let model_id = state
        .orchestrator
        .model_for(engine_id, aichip_shared::ModelTier::Complex);

    // No tools and no project directory: this writes text, and a run that
    // cannot touch the filesystem cannot get that wrong in an interesting way.
    let output = aichip_core::runs::utility::utility_run(
        engine,
        model_id,
        apps::scaffold::prompt(description),
        Some(aichip_shared::ReasoningEffort::High),
        std::time::Duration::from_secs(180),
    )
    .await
    .map_err(internal)?;

    let manifest = apps::scaffold::extract(&output);
    let error = apps::manifest::parse(&manifest).err().map(|e| e.to_string());
    Ok(Json(json!({ "manifest": manifest, "error": error })))
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
    let (_, outcome) = apps::set_manifest(&state.db, &app, &body.manifest)
        .await
        .map_err(bad_manifest)?;
    let app = load(&state, id).await?;
    let mut out = app_json(&app);
    out["manifest"] = json!(app.manifest);
    // The caller needs to know whether the tables followed. A manifest that
    // saved but whose columns are waiting on approval looks identical
    // otherwise, and the app would appear to have changed when it has not.
    out["applied"] = json!(outcome.applied.len());
    out["pending"] = outcome.pending.as_ref().map_or(Value::Null, plan_json);
    Ok(Json(out))
}

/// The migration this app is waiting on, if any.
async fn schema_plan(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    load(&state, id).await?;
    let pending = apps::pending_plan(&state.db, id).await.map_err(internal)?;
    Ok(Json(json!({ "pending": pending.as_ref().map_or(Value::Null, plan_json) })))
}

#[derive(Deserialize)]
struct PlanId {
    plan_id: Uuid,
}

/// Run a migration a person has read.
///
/// The plan id is required rather than "whatever is pending", so approving a
/// screen someone has been looking at cannot silently apply a different
/// migration that replaced it while they read.
async fn apply_schema(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PlanId>,
) -> Result<Json<Value>, ApiError> {
    load(&state, id).await?;
    let applied = apps::apply_plan(&state.db, body.plan_id)
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({ "applied": applied.len() })))
}

async fn discard_schema(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PlanId>,
) -> Result<Json<Value>, ApiError> {
    load(&state, id).await?;
    apps::discard_plan(&state.db, body.plan_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "ok": true })))
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

// ── An app's own rows ───────────────────────────────────────────────────────
//
// No scope gates these. The tables exist because this app declared them, hold
// only what it put there, and are dropped with it — asking permission to use
// the thing you installed would be ceremony.

/// A data problem is the caller's to fix: an undeclared field, a value that is
/// not a number, a filter that is not a filter. 400 with the text, which names
/// which field and what was sent.
fn bad_data(e: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, e.to_string())
}

/// Resolve an app and one of its models together.
///
/// Both come from the manifest, so nothing after this point is holding a name
/// that arrived in the URL — which is the property the whole data layer rests
/// on. See `apps::query` for why.
async fn model_of(
    state: &AppState,
    id: Uuid,
    model: &str,
) -> Result<(apps::App, apps::manifest::Model), ApiError> {
    let app = load(state, id).await?;
    let parsed = app.parsed().map_err(bad_manifest)?;
    let model = apps::data::model_of(&parsed, model)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?
        .clone();
    Ok((app, model))
}

/// Read the query string as a list of pairs.
///
/// A list, not a struct: `where` may appear several times and a struct keeps
/// only the last one, which would silently drop every filter but one and
/// return the wrong rows without erroring.
fn raw_query(pairs: Vec<(String, String)>) -> apps::query::Raw {
    let mut raw = apps::query::Raw::default();
    for (key, value) in pairs {
        match key.as_str() {
            "where" => raw.filters.push(value),
            "order" => raw.order = Some(value),
            "limit" => raw.limit = value.parse().ok(),
            "offset" => raw.offset = value.parse().ok(),
            _ => {}
        }
    }
    raw
}

async fn rows(
    State(state): State<AppState>,
    Path((id, model)): Path<(Uuid, String)>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<Json<Value>, ApiError> {
    let (app, model) = model_of(&state, id, &model).await?;
    let raw = raw_query(pairs);
    let rows = apps::data::list(&state.db, &app.schema, &model, &raw)
        .await
        .map_err(bad_data)?;
    let total = apps::data::count(&state.db, &app.schema, &model, &raw)
        .await
        .map_err(bad_data)?;
    Ok(Json(json!({ "rows": rows, "total": total })))
}

async fn row(
    State(state): State<AppState>,
    Path((id, model, row)): Path<(Uuid, String, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let (app, model) = model_of(&state, id, &model).await?;
    apps::data::get(&state.db, &app.schema, &model, row)
        .await
        .map_err(bad_data)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "no such row".into()))
}

async fn add_row(
    State(state): State<AppState>,
    Path((id, model)): Path<(Uuid, String)>,
    Json(body): Json<serde_json::Map<String, Value>>,
) -> Result<Json<Value>, ApiError> {
    let (app, model) = model_of(&state, id, &model).await?;
    apps::data::create(&state.db, &app.schema, &model, &body)
        .await
        .map(Json)
        .map_err(bad_data)
}

async fn change_row(
    State(state): State<AppState>,
    Path((id, model, row)): Path<(Uuid, String, Uuid)>,
    Json(body): Json<serde_json::Map<String, Value>>,
) -> Result<Json<Value>, ApiError> {
    let (app, model) = model_of(&state, id, &model).await?;
    apps::data::update(&state.db, &app.schema, &model, row, &body)
        .await
        .map_err(bad_data)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "no such row".into()))
}

async fn drop_row(
    State(state): State<AppState>,
    Path((id, model, row)): Path<(Uuid, String, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let (app, model) = model_of(&state, id, &model).await?;
    let gone = apps::data::delete(&state.db, &app.schema, &model, row)
        .await
        .map_err(bad_data)?;
    if gone {
        Ok(Json(json!({ "ok": true })))
    } else {
        Err((StatusCode::NOT_FOUND, "no such row".into()))
    }
}

/// The buckets behind a chart view.
///
/// Grouped in Postgres. A year of entries drawn as twelve bars should send
/// twelve numbers, not a year of entries.
async fn chart(
    State(state): State<AppState>,
    Path((id, view)): Path<(Uuid, String)>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<Json<Value>, ApiError> {
    let app = load(&state, id).await?;
    let parsed = app.parsed().map_err(bad_manifest)?;
    let view = parsed
        .view(&view)
        .ok_or((StatusCode::NOT_FOUND, "no such view".to_string()))?;
    let apps::manifest::ViewSpec::Chart { group_by, measure, .. } = &view.spec else {
        return Err((StatusCode::BAD_REQUEST, "that view is not a chart".into()));
    };
    let model = parsed
        .model(&view.model)
        .ok_or((StatusCode::NOT_FOUND, "no such model".to_string()))?;

    let buckets = apps::render::chart(
        &state.db,
        &app.schema,
        model,
        group_by,
        measure,
        &raw_query(pairs),
    )
    .await
    .map_err(bad_data)?;
    Ok(Json(json!({ "buckets": buckets })))
}

async fn load(state: &AppState, id: Uuid) -> Result<apps::App, ApiError> {
    apps::get(&state.db, id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such app".into()))
}
