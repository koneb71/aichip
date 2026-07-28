use super::{internal, ApiError};
use crate::AppState;
use aichip_core::runs::utility::{extract_json, utility_run};
use aichip_shared::{ModelTier, ReasoningEffort};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agents", get(list).post(create))
        .route("/agents/generate", post(generate))
        .route("/agents/{id}", patch(update).delete(remove))
        .route("/agents/{id}/memories", get(memories))
        .route("/agent-memories/{id}", axum::routing::delete(forget))
}

/// What this agent remembers, newest first — shown in the agent drawer so the
/// user can see (and prune) what will be fed into its next runs.
async fn memories(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT m.id, m.kind, m.content, m.created_at, p.name AS project_name
         FROM agent_memories m LEFT JOIN projects p ON p.id = m.project_id
         WHERE m.agent_id=$1 ORDER BY m.created_at DESC LIMIT 50",
    )
    .bind(agent_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "memories": rows.iter().map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "kind": r.get::<String, _>("kind"),
            "content": r.get::<String, _>("content"),
            "projectName": r.get::<Option<String>, _>("project_name"),
            "ts": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        })).collect::<Vec<_>>()
    })))
}

/// Forget one memory. The user owns the agent's memory, not the agent.
async fn forget(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    sqlx::query("DELETE FROM agent_memories WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}

fn agent_json(r: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": r.get::<Uuid, _>("id"),
        "name": r.get::<String, _>("name"),
        "icon": r.get::<String, _>("icon"),
        "color": r.get::<String, _>("color"),
        "description": r.get::<String, _>("description"),
        "systemPrompt": r.get::<String, _>("system_prompt"),
        "modelTier": r.get::<String, _>("model_tier"),
        "allowedTools": r.get::<Vec<String>, _>("allowed_tools"),
        "permissionPreset": r.get::<String, _>("permission_preset"),
        "effort": r.get::<Option<String>, _>("effort"),
        "builtin": r.get::<bool, _>("builtin"),
    })
}

#[derive(Deserialize)]
struct WsFilter {
    workspace_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<WsFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT * FROM agents WHERE $1::uuid IS NULL OR workspace_id = $1 ORDER BY created_at ASC",
    )
    .bind(filter.workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({ "agents": rows.iter().map(agent_json).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
struct AgentBody {
    workspace_id: Uuid,
    name: String,
    #[serde(default = "default_icon")]
    icon: String,
    #[serde(default = "default_color")]
    color: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    model_tier: ModelTier,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default = "default_preset")]
    permission_preset: String,
    /// None leaves the CLI's own default alone.
    #[serde(default)]
    effort: Option<String>,
}

fn default_icon() -> String {
    "bot".into()
}
fn default_color() -> String {
    "#4f46e5".into()
}
fn default_preset() -> String {
    "reviewed".into()
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<AgentBody>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let tier = serde_json::to_value(body.model_tier).unwrap();
    let row = sqlx::query(
        "INSERT INTO agents (workspace_id, name, icon, color, description, system_prompt,
                             model_tier, allowed_tools, permission_preset, effort)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING *",
    )
    .bind(body.workspace_id)
    .bind(body.name.trim())
    .bind(&body.icon)
    .bind(&body.color)
    .bind(&body.description)
    .bind(&body.system_prompt)
    .bind(tier.as_str().unwrap())
    .bind(&body.allowed_tools)
    .bind(&body.permission_preset)
    .bind(body.effort.as_deref().filter(|e| !e.is_empty()))
    .fetch_one(&state.db.pool)
    .await
    .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(agent_json(&row)))
}

#[derive(Deserialize)]
struct AgentPatch {
    name: Option<String>,
    icon: Option<String>,
    color: Option<String>,
    description: Option<String>,
    system_prompt: Option<String>,
    model_tier: Option<ModelTier>,
    allowed_tools: Option<Vec<String>>,
    permission_preset: Option<String>,
    /// Present-but-null clears it back to the CLI default.
    #[serde(default, deserialize_with = "double_option")]
    effort: Option<Option<String>>,
}

/// Distinguish "field absent" from "field set to null" so clearing works.
fn double_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(de).map(Some)
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<AgentPatch>,
) -> Result<Json<Value>, ApiError> {
    let tier = body
        .model_tier
        .map(|t| serde_json::to_value(t).unwrap().as_str().unwrap().to_string());
    let row = sqlx::query(
        "UPDATE agents SET
            name = COALESCE($1, name), icon = COALESCE($2, icon),
            color = COALESCE($3, color), description = COALESCE($4, description),
            system_prompt = COALESCE($5, system_prompt), model_tier = COALESCE($6, model_tier),
            allowed_tools = COALESCE($7, allowed_tools),
            permission_preset = COALESCE($8, permission_preset),
            effort = CASE WHEN $10 THEN $9 ELSE effort END
         WHERE id = $11 RETURNING *",
    )
    .bind(body.name)
    .bind(body.icon)
    .bind(body.color)
    .bind(body.description)
    .bind(body.system_prompt)
    .bind(tier)
    .bind(body.allowed_tools)
    .bind(body.permission_preset)
    .bind(body.effort.clone().flatten().filter(|e| !e.is_empty()))
    .bind(body.effort.is_some())
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(agent_json(&row)))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    sqlx::query("DELETE FROM agents WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
struct GenerateBody {
    description: String,
    /// "claude-code" (default) or "mock" for tests.
    engine: Option<String>,
}

const GENERATE_PROMPT: &str = r##"You are designing coding agents for a multi-agent workflow platform.
Based on the user's need below, output ONLY a JSON array of 1 to 4 agent definitions — no prose,
no markdown fences. Each element:
{"name": "short name", "icon": "one of: bot|wrench|shield|book|flask|scale",
 "color": "#hex", "description": "one line",
 "system_prompt": "2-6 sentences defining role, approach, and output standards",
 "model_tier": "easy"|"medium"|"complex",
 "permission_preset": "reviewed"|"auto_edit",
 "allowed_tools": []}
Tier guide: easy=mechanical work (Sonnet), medium=typical coding (Opus), complex=review/judging/architecture (Fable).

User's need: "##;

async fn generate(
    State(state): State<AppState>,
    Json(body): Json<GenerateBody>,
) -> Result<Json<Value>, ApiError> {
    if body.description.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "description is required".into()));
    }
    let engine_id = body.engine.as_deref().unwrap_or("claude-code");
    let engine = state
        .orchestrator
        .engine(engine_id)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, format!("unknown engine {engine_id}")))?;
    let model_id = state
        .orchestrator
        .tiers
        .model_for(ModelTier::Complex)
        .to_string();
    let prompt = format!("{GENERATE_PROMPT}{}\"", body.description.trim());

    // Designing a team is the kind of one-shot judgement that repays
    // thinking time far more than it repays a bigger model.
    let output = utility_run(
        engine,
        model_id,
        prompt,
        Some(ReasoningEffort::High),
        Duration::from_secs(180),
    )
        .await
        .map_err(internal)?;
    match extract_json(&output) {
        Ok(Value::Array(drafts)) => Ok(Json(json!({ "drafts": drafts }))),
        Ok(single @ Value::Object(_)) => Ok(Json(json!({ "drafts": [single] }))),
        _ => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("model output was not valid JSON:\n{output}"),
        )),
    }
}
