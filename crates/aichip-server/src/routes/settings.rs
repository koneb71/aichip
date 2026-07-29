//! Settings that belong to the machine rather than to a workspace.
//!
//! Today that means which model each complexity tier routes to. The tiers
//! themselves (easy / medium / complex) are how tasks, agents and workflow
//! steps express "how hard is this" — but which model answers that question
//! depends on the user's plan and appetite, and used to be hard-coded.

use super::{internal, ApiError};
use crate::AppState;
use aichip_shared::{is_known_model, ModelTier, TierMapping, MODEL_CHOICES};
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new().route("/settings/models", get(get_models).put(set_models))
}

async fn get_models(State(state): State<AppState>) -> Json<Value> {
    let mapping = state.orchestrator.tier_mapping();
    Json(json!({
        "tiers": {
            "easy": mapping.model_for(ModelTier::Easy),
            "medium": mapping.model_for(ModelTier::Medium),
            "complex": mapping.model_for(ModelTier::Complex),
        },
        // The catalog travels with the current values so the picker can't
        // offer a model the server would then reject.
        "choices": MODEL_CHOICES.iter().map(|m| json!({
            "id": m.id, "label": m.label, "blurb": m.blurb,
        })).collect::<Vec<_>>(),
        "defaults": {
            "easy": TierMapping::default().model_for(ModelTier::Easy),
            "medium": TierMapping::default().model_for(ModelTier::Medium),
            "complex": TierMapping::default().model_for(ModelTier::Complex),
        },
    }))
}

#[derive(Deserialize)]
struct ModelsBody {
    easy: String,
    medium: String,
    complex: String,
}

async fn set_models(
    State(state): State<AppState>,
    Json(body): Json<ModelsBody>,
) -> Result<Json<Value>, ApiError> {
    // Checked here as well as in the orchestrator so the message names the
    // offending field rather than just the id.
    for (field, model) in [
        ("easy", &body.easy),
        ("medium", &body.medium),
        ("complex", &body.complex),
    ] {
        if !is_known_model(model) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("\"{model}\" is not a model aichip offers (field {field})"),
            ));
        }
    }

    let mapping = TierMapping(
        [
            (ModelTier::Easy, body.easy),
            (ModelTier::Medium, body.medium),
            (ModelTier::Complex, body.complex),
        ]
        .into_iter()
        .collect(),
    );

    state
        .orchestrator
        .set_tier_mapping(mapping)
        .await
        .map_err(internal)?;

    // Runs already in flight keep the model they started with — a model is
    // chosen when a run starts, and the CLI cannot be asked to switch mid
    // conversation.
    Ok(Json(json!({ "saved": true })))
}
