//! Skills: named ways of doing something, and the harness that lets you try
//! one before it does real work.

use super::{internal, ApiError};
use crate::AppState;
use aichip_core::runs::utility::utility_run;
use aichip_shared::{ModelTier, ReasoningEffort};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/skills", get(list).post(create))
        .route("/skills/{id}", patch(update).delete(remove))
        .route("/skills/{id}/try", post(try_it))
        // Installing is project-shaped even though the library is
        // workspace-shaped: the files land in one checkout, and that checkout
        // is what an agent reads.
        .route("/projects/{id}/skills/install", post(install))
}

#[derive(Deserialize)]
struct WorkspaceFilter {
    workspace_id: Uuid,
}

async fn list(
    State(state): State<AppState>,
    Query(q): Query<WorkspaceFilter>,
) -> Result<Json<Value>, ApiError> {
    let skills = aichip_core::skills::list(&state.db, q.workspace_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "skills": skills })))
}

#[derive(Deserialize)]
struct SkillBody {
    workspace_id: Option<Uuid>,
    name: Option<String>,
    description: Option<String>,
    instructions: Option<String>,
    must_not: Option<String>,
    enabled: Option<bool>,
}

/// Everything a person types here ends up in a prompt, so it gets the same
/// check the Brain does before anything is stored.
fn no_secrets(body: &SkillBody) -> Result<(), ApiError> {
    for text in [&body.instructions, &body.must_not, &body.description] {
        if let Some(found) = text.as_deref().and_then(aichip_shared::looks_like_secret) {
            return Err((
                StatusCode::BAD_REQUEST,
                aichip_shared::secrets::refusal(&found),
            ));
        }
    }
    Ok(())
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<SkillBody>,
) -> Result<Json<Value>, ApiError> {
    let workspace_id = body.workspace_id.ok_or((
        StatusCode::BAD_REQUEST,
        "workspace_id is required".to_string(),
    ))?;
    let name = body.name.as_deref().unwrap_or("").trim().to_string();
    no_secrets(&body)?;

    // One `@` namespace with agents, so this refusal has to name the reason —
    // "that already exists" would leave somebody hunting through the wrong list.
    if let Err(why) = aichip_core::skills::check_name_free(&state.db, workspace_id, &name, None)
        .await
        .map_err(internal)?
    {
        return Err((StatusCode::CONFLICT, why));
    }

    let row = sqlx::query(
        "INSERT INTO skills (workspace_id, name, description, instructions, must_not, enabled)
         VALUES ($1,$2,$3,$4,$5,$6) RETURNING id",
    )
    .bind(workspace_id)
    .bind(&name)
    .bind(body.description.as_deref().unwrap_or(""))
    .bind(body.instructions.as_deref().unwrap_or(""))
    .bind(body.must_not.as_deref().unwrap_or(""))
    .bind(body.enabled.unwrap_or(true))
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    let id: Uuid = sqlx::Row::get(&row, "id");
    one(&state, id).await
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SkillBody>,
) -> Result<Json<Value>, ApiError> {
    no_secrets(&body)?;

    if let Some(name) = body.name.as_deref().map(str::trim) {
        let workspace_id: Uuid = sqlx::query_scalar("SELECT workspace_id FROM skills WHERE id=$1")
            .bind(id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?
            .ok_or((StatusCode::NOT_FOUND, "no such skill".to_string()))?;
        if let Err(why) =
            aichip_core::skills::check_name_free(&state.db, workspace_id, name, Some(id))
                .await
                .map_err(internal)?
        {
            return Err((StatusCode::CONFLICT, why));
        }
    }

    sqlx::query(
        "UPDATE skills SET name = COALESCE($2, name),
                           description = COALESCE($3, description),
                           instructions = COALESCE($4, instructions),
                           must_not = COALESCE($5, must_not),
                           enabled = COALESCE($6, enabled),
                           updated_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.description.as_deref())
    .bind(body.instructions.as_deref())
    .bind(body.must_not.as_deref())
    .bind(body.enabled)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    one(&state, id).await
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    sqlx::query("DELETE FROM skills WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}

async fn one(state: &AppState, id: Uuid) -> Result<Json<Value>, ApiError> {
    let skill = aichip_core::skills::get(&state.db, id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such skill".to_string()))?;
    Ok(Json(serde_json::to_value(skill).unwrap_or_default()))
}

#[derive(Deserialize)]
struct TryBody {
    /// The low-risk prompt to try it against.
    prompt: String,
    model_tier: Option<ModelTier>,
}

/// Try a skill before it does real work.
///
/// The source this feature is modelled on is blunt about it — *"a skill should
/// be tested with a simple prompt before it becomes part of real work"*, with a
/// prompt that avoids file operations, shell commands and credentials. This is
/// that, and it is safe by construction rather than by asking nicely:
/// `utility_run` has **no tools at all**, no worktree, no repository and no MCP
/// wiring, so whatever the skill says to do, there is nothing here to do it to.
/// What comes back is text, and it is shown rather than stored.
///
/// Which is also the honest limit, and the UI says so: this tells you how the
/// skill *reads*, not what it would do to your files.
async fn try_it(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<TryBody>,
) -> Result<Json<Value>, ApiError> {
    if body.prompt.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "give it something to try — a short, harmless request".into(),
        ));
    }
    let skill = aichip_core::skills::get(&state.db, id)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such skill".to_string()))?;

    let engine_id = state.orchestrator.default_engine();
    let engine = state
        .orchestrator
        .engine(&engine_id)
        .ok_or((StatusCode::BAD_REQUEST, format!("no engine {engine_id}")))?;
    let tier = body.model_tier.unwrap_or(ModelTier::Medium);
    let model_id = state.orchestrator.model_for(&engine_id, tier);

    // Assembled exactly as a real run would assemble it — the point of a test
    // is that it exercises the same text, fence and all.
    let prompt = aichip_core::skills::augment_prompt(body.prompt.trim(), Some(&skill));

    let output = utility_run(
        engine,
        model_id,
        prompt.clone(),
        Some(ReasoningEffort::Low),
        Duration::from_secs(120),
    )
    .await
    .map_err(internal)?;

    Ok(Json(json!({
        "output": output,
        // Shown alongside, because half of what a test tells you is whether the
        // skill says what you thought it said.
        "prompt": prompt,
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallBody {
    /// `owner/repo`, a GitHub URL, or a skills.sh page. Normalised and
    /// refused in the core rather than passed on for the installer to fail at.
    reference: String,
}

/// Install a skill from a registry into this project, and mirror it.
///
/// Slow on purpose — `npx` fetches a package and then a repository, which
/// takes as long as it takes. The button says so rather than the request
/// pretending to be quick and the person pressing it twice.
async fn install(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<InstallBody>,
) -> Result<Json<Value>, ApiError> {
    let out = aichip_core::skills::install::install(&state.db, project_id, &body.reference)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::to_value(out).map_err(internal)?))
}
