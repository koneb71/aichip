//! Settings that belong to the machine rather than to a workspace.
//!
//! Today that means which model each complexity tier routes to. The tiers
//! themselves (easy / medium / complex) are how tasks, agents and workflow
//! steps express "how hard is this" — but which model answers that question
//! depends on the user's plan and appetite, and used to be hard-coded.

use super::{internal, ApiError};
use crate::AppState;
use aichip_shared::{
    is_known_model_for, EngineTierMapping, ModelTier, PermissionMode, ReasoningEffort, TierMapping,
    MODEL_CHOICES,
};
use std::collections::BTreeMap;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings/models", get(get_models).put(set_models))
        .route("/settings/permissions", get(get_permissions).put(set_permissions))
        .route("/settings/effort", get(get_effort).put(set_effort))
        .route("/settings/permissions/apply-to-agents", axum::routing::post(apply_to_agents))
}

async fn get_models(State(state): State<AppState>) -> Json<Value> {
    let mapping = state.orchestrator.tier_mapping();
    // One column per engine that is actually installed. An engine the user
    // doesn't have is not worth asking them to configure.
    let engines = state
        .orchestrator
        .engines()
        .into_iter()
        .filter(|e| e.id() != "mock")
        .map(|e| {
            let current = mapping.for_engine(e.id());
            let defaults = EngineTierMapping::defaults_for(e.id());
            json!({
                "id": e.id(),
                "label": e.label(),
                // A fixed catalog is only honest for an engine that has one.
                // OpenCode fronts 75+ providers, so its field is free text and
                // the picker says so rather than offering a stale list.
                "fixedCatalog": e.capabilities().fixed_model_catalog,
                "choices": if e.capabilities().fixed_model_catalog {
                    MODEL_CHOICES.iter().map(|m| json!({
                        "id": m.id, "label": m.label, "blurb": m.blurb,
                    })).collect::<Vec<_>>()
                } else {
                    vec![]
                },
                // What this install can actually reach, straight from the CLI.
                // Suggestions, not a whitelist: a local model the CLI doesn't
                // enumerate is still a legitimate thing to type.
                "available": state
                    .orchestrator
                    .engine_info(e.id())
                    .map(|i| i.models.clone())
                    .unwrap_or_default(),
                "providers": state
                    .orchestrator
                    .engine_info(e.id())
                    .map(|i| i.providers.clone())
                    .unwrap_or_default(),
                "tiers": tiers_json(&current),
                "defaults": tiers_json(&defaults),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({ "engines": engines }))
}

fn tiers_json(m: &TierMapping) -> Value {
    json!({
        "easy": m.model_for(ModelTier::Easy),
        "medium": m.model_for(ModelTier::Medium),
        "complex": m.model_for(ModelTier::Complex),
    })
}

#[derive(Deserialize)]
struct ModelsBody {
    /// `{engine_id: {easy, medium, complex}}`.
    engines: BTreeMap<String, TierRow>,
}

#[derive(Deserialize)]
struct TierRow {
    easy: String,
    medium: String,
    complex: String,
}

async fn set_models(
    State(state): State<AppState>,
    Json(body): Json<ModelsBody>,
) -> Result<Json<Value>, ApiError> {
    let mut mapping = BTreeMap::new();
    for (engine, row) in body.engines {
        if state.orchestrator.engine(&engine).is_none() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("\"{engine}\" is not an engine this machine has"),
            ));
        }
        // Checked here as well as in the orchestrator so the message names the
        // offending field rather than just the id.
        for (field, model) in [
            ("easy", &row.easy),
            ("medium", &row.medium),
            ("complex", &row.complex),
        ] {
            if !is_known_model_for(&engine, model) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("\"{model}\" is not a model {engine} can run (field {field})"),
                ));
            }
        }
        mapping.insert(
            engine,
            TierMapping(
                [
                    (ModelTier::Easy, row.easy),
                    (ModelTier::Medium, row.medium),
                    (ModelTier::Complex, row.complex),
                ]
                .into_iter()
                .collect(),
            ),
        );
    }

    state
        .orchestrator
        .set_tier_mapping(EngineTierMapping(mapping))
        .await
        .map_err(internal)?;

    // Runs already in flight keep the model they started with — a model is
    // chosen when a run starts, and the CLI cannot be asked to switch mid
    // conversation.
    Ok(Json(json!({ "saved": true })))
}

/// How much freedom new work gets by default.
async fn get_permissions(State(state): State<AppState>) -> Json<Value> {
    // How many agents pin their own mode. A bound agent's preset overrides
    // the default, so this number is exactly "how many agents will ignore
    // the setting above" — worth showing rather than leaving to be discovered
    // when a run stops to ask anyway.
    let overriding: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agents WHERE permission_preset IS NOT NULL",
    )
    .fetch_one(&state.db.pool)
    .await
    .unwrap_or(0);

    Json(json!({
        "agentsOverriding": overriding,
        "defaultMode": state.orchestrator.default_permission_mode().await,
        "modes": [
            { "id": "reviewed",  "label": "Ask me first",
              "blurb": "Every file edit and command waits for your approval." },
            { "id": "auto_edit", "label": "Edit freely, ask before commands",
              "blurb": "File changes go ahead; shell commands still stop for you." },
            { "id": "full_auto", "label": "Don't ask",
              "blurb": "Work straight through. Needs a git project that opted in, because the run is isolated in a worktree you review before merging." },
        ],
    }))
}

#[derive(Deserialize)]
struct PermissionsBody {
    default_mode: PermissionMode,
}

#[derive(Deserialize)]
struct EffortBody {
    /// Absent or null returns every run to its CLI's own default rather than
    /// pinning one, which is a real choice and the one this ships with.
    default_effort: Option<ReasoningEffort>,
}

async fn get_effort(State(state): State<AppState>) -> Json<Value> {
    // Agents carrying their own budget outrank this, exactly as they do for
    // permissions — worth counting here rather than leaving it to be discovered
    // when a card thinks harder or less hard than the setting says.
    let overriding: i64 = sqlx::query_scalar("SELECT count(*) FROM agents WHERE effort IS NOT NULL")
        .fetch_one(&state.db.pool)
        .await
        .unwrap_or(0);
    Json(json!({
        "agentsOverriding": overriding,
        "defaultEffort": state.orchestrator.default_effort().await,
        "levels": [
            { "id": "low",    "label": "Low",     "blurb": "Fast and cheap. Fine for mechanical edits." },
            { "id": "medium", "label": "Medium",  "blurb": "A balance. Sensible for most work." },
            { "id": "high",   "label": "High",    "blurb": "Thinks before acting. Worth it for design and debugging." },
            { "id": "xhigh",  "label": "Extra high", "blurb": "Slower and dearer; for genuinely hard problems." },
            { "id": "max",    "label": "Maximum", "blurb": "As much as the CLI will give. Expect long waits." }
        ]
    }))
}

async fn set_effort(
    State(state): State<AppState>,
    Json(body): Json<EffortBody>,
) -> Result<Json<Value>, ApiError> {
    state
        .orchestrator
        .set_default_effort(body.default_effort)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "defaultEffort": body.default_effort })))
}

async fn set_permissions(
    State(state): State<AppState>,
    Json(body): Json<PermissionsBody>,
) -> Result<Json<Value>, ApiError> {
    state
        .orchestrator
        .set_default_permission_mode(body.default_mode)
        .await
        .map_err(internal)?;
    // Note this is only the *default* for new cards. "Don't ask" still needs
    // the per-project opt-in, and the orchestrator downgrades it outside an
    // aichip-managed worktree regardless of what is stored here.
    Ok(Json(json!({ "defaultMode": body.default_mode })))
}

/// Clear every agent's own preset so they all follow the workspace default.
///
/// Without this, changing the default appears to do nothing: most cards have
/// an agent, and an agent's preset wins. Rather than silently rewriting
/// agents when the default changes — which would quietly widen what a
/// carefully-configured agent may do — this is an explicit action the user
/// takes once.
async fn apply_to_agents(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let cleared = sqlx::query("UPDATE agents SET permission_preset = NULL WHERE permission_preset IS NOT NULL")
        .execute(&state.db.pool)
        .await
        .map_err(internal)?
        .rows_affected();
    Ok(Json(json!({ "cleared": cleared })))
}
