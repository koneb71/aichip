use super::{attachments, internal, ApiError};
use crate::AppState;
use aichip_core::runs::orchestrator::Variant;
use aichip_shared::{ModelTier, PermissionMode};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tasks", get(list).post(create))
        .route("/tasks/{id}", axum::routing::patch(move_task).delete(delete_task))
        .route("/tasks/{id}/retry", post(retry))
        .route("/tasks/{id}/comments", get(comments).post(post_comment))
        .route("/tasks/{id}/articles", get(task_articles).put(set_task_articles))
        .route("/tasks/{id}/attachments/claim", post(attach_to_task))
        .route("/tasks/{id}/start", post(start))
        .route("/tasks/{id}/bakeoff", get(bakeoff).post(start_bakeoff))
        .route("/runs/{id}/keep", post(keep_variant))
        .route("/tasks/{id}/diff", get(diff))
        .route("/tasks/{id}/merge", post(merge))
        .route("/runs/{id}/events", get(run_events))
        .route("/runs/{id}/pending-permissions", get(pending_permissions))
        .route("/runs/{id}/cancel", post(cancel_run))
        .route("/runs/{id}/plan", get(plan).patch(edit_plan))
        .route("/runs/{id}/plan/approve", post(approve_plan))
        .route("/runs/{id}/plan/revise", post(revise_plan))
        .route("/permissions/{request_id}/resolve", post(resolve_permission))
}

#[derive(Deserialize)]
struct TaskFilter {
    workspace_id: Option<Uuid>,
    project_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<TaskFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        // The epic columns ride along on the one query the board already makes.
        // Counting children per card from the client would be an N+1 over a list
        // that refreshes every 2.5 seconds.
        //
        // The roll-up counts `board_column`, not step status, on purpose: it has
        // to keep telling the truth after the org run — and its steps — have been
        // deleted, which is exactly when someone is looking back at what an epic
        // turned into.
        "SELECT t.id, t.title, t.prompt, t.model_tier, t.board_column, t.branch, t.position,
                t.project_id, t.agent_id, COALESCE(a.engine, t.engine) AS engine, t.plan_first,
                a.name AS agent_name, a.color AS agent_color,
                t.team_id, tm.name AS team_name, tm.pattern AS team_pattern,
                t.parent_id, parent.title AS parent_title,
                COALESCE(kids.total, 0) AS child_count,
                COALESCE(kids.resolved, 0) AS child_resolved,
                s.status AS step_status,
                -- The mode this card will actually run under, and which of the
                -- three places decided it. Resolved here in exactly the order
                -- the orchestrator resolves it (orchestrator.rs:1090) so the
                -- board cannot disagree with what the run does.
                --
                -- Worth showing at all because the precedence surprises people:
                -- a project set to work without asking still prompts when the
                -- bound agent carries its own preset, and nothing said so.
                COALESCE(a.permission_preset, t.permission_mode,
                         (SELECT value #>> '{}' FROM settings
                           WHERE key = 'default_permission_mode'),
                         'reviewed') AS effective_mode,
                CASE WHEN a.permission_preset IS NOT NULL THEN 'agent'
                     WHEN t.permission_mode IS NOT NULL THEN 'card'
                     ELSE 'default' END AS permission_source,
                r.id AS run_id, r.status AS run_status, r.cost_usd, r.model,
                r.team_id AS run_team_id
         FROM tasks t
         JOIN projects p ON p.id = t.project_id
         LEFT JOIN agents a ON a.id = t.agent_id
         LEFT JOIN teams tm ON tm.id = t.team_id
         LEFT JOIN tasks parent ON parent.id = t.parent_id
         LEFT JOIN steps s ON s.task_id = t.id
         LEFT JOIN LATERAL (
             SELECT count(*) AS total,
                    count(*) FILTER (WHERE board_column IN ('review','done')) AS resolved
             FROM tasks c WHERE c.parent_id = t.id
         ) kids ON TRUE
         LEFT JOIN LATERAL (
             SELECT * FROM runs WHERE task_id = t.id ORDER BY created_at DESC LIMIT 1
         ) r ON TRUE
         WHERE ($1::uuid IS NULL OR p.workspace_id = $1)
           AND ($2::uuid IS NULL OR t.project_id = $2)
         ORDER BY t.position, t.created_at",
    )
    .bind(filter.workspace_id)
    .bind(filter.project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let tasks: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid, _>("id"),
                "title": r.get::<String, _>("title"),
                "modelTier": r.get::<String, _>("model_tier"),
                "boardColumn": r.get::<String, _>("board_column"),
                "position": r.get::<f64, _>("position"),
                "branch": r.get::<Option<String>, _>("branch"),
                "projectId": r.get::<Uuid, _>("project_id"),
                "agentId": r.get::<Option<Uuid>, _>("agent_id"),
                "agentName": r.get::<Option<String>, _>("agent_name"),
                "agentColor": r.get::<Option<String>, _>("agent_color"),
                "teamId": r.get::<Option<Uuid>, _>("team_id"),
                "teamName": r.get::<Option<String>, _>("team_name"),
                "teamPattern": r.get::<Option<String>, _>("team_pattern"),
                "parentId": r.get::<Option<Uuid>, _>("parent_id"),
                "parentTitle": r.get::<Option<String>, _>("parent_title"),
                "childCount": r.get::<i64, _>("child_count"),
                "childResolved": r.get::<i64, _>("child_resolved"),
                // The raw assignment status, so the card can say "failed" or
                // "dropped" — things the four columns have no room for.
                "stepStatus": r.get::<Option<String>, _>("step_status"),
                "effectiveMode": r.get::<String, _>("effective_mode"),
                "permissionSource": r.get::<String, _>("permission_source"),
                "orgRunId": r.get::<Option<Uuid>, _>("run_team_id")
                    .and(r.get::<Option<Uuid>, _>("run_id")),
                "runId": r.get::<Option<Uuid>, _>("run_id"),
                "runStatus": r.get::<Option<String>, _>("run_status"),
                "costUsd": r.get::<Option<f64>, _>("cost_usd"),
                "model": r.get::<Option<String>, _>("model"),
                "engine": r.get::<String, _>("engine"),
                "planFirst": r.get::<bool, _>("plan_first"),
            })
        })
        .collect();
    Ok(Json(json!({ "tasks": tasks })))
}

#[derive(Deserialize)]
struct CreateTask {
    project_id: Uuid,
    title: String,
    prompt: String,
    #[serde(default)]
    model_tier: ModelTier,
    /// Absent means "use the workspace default", which is not the same as
    /// asking for Reviewed — `#[serde(default)]` here would silently force
    /// prompts on every client that doesn't name a mode.
    permission_mode: Option<PermissionMode>,
    #[serde(default)]
    start: bool,
    agent_id: Option<Uuid>,
    /// Hand the whole task to a team instead of a single agent.
    team_id: Option<Uuid>,
    /// Engine id from `/api/engines`. Omitted means the machine default.
    engine: Option<String>,
    /// Write a plan and stop, so a person can confirm or rewrite it before any
    /// work happens.
    #[serde(default)]
    plan_first: bool,
    /// Knowledge-base articles the agent should read before starting.
    #[serde(default)]
    article_ids: Vec<Uuid>,
    /// Ids from POST /api/projects/{id}/attachments, bound to this task on
    /// create. Defaulted so existing clients keep working.
    #[serde(default)]
    attachment_ids: Vec<Uuid>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateTask>,
) -> Result<Json<Value>, ApiError> {
    let tier = serde_json::to_value(body.model_tier).unwrap();
    // Store NULL when the caller didn't choose, so the card inherits whatever
    // the default is *when it runs* rather than freezing today's value.
    let mode: Option<String> = body
        .permission_mode
        .map(|m| serde_json::to_value(m).unwrap().as_str().unwrap().to_string());
    let row = sqlx::query(
        "INSERT INTO tasks (project_id, title, prompt, model_tier, permission_mode, engine, agent_id, team_id, board_column, plan_first)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'backlog',$9) RETURNING id",
    )
    .bind(body.project_id)
    .bind(&body.title)
    .bind(&body.prompt)
    .bind(tier.as_str().unwrap())
    .bind(mode.as_deref())
    .bind(body.engine.clone().unwrap_or_else(|| state.orchestrator.default_engine()))
    .bind(body.agent_id)
    .bind(body.team_id)
    .bind(body.plan_first)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let task_id: Uuid = row.get("id");
    // Before the run is enqueued, for the same reason attachments are: the
    // prompt is assembled from whatever is bound when the run is picked up.
    link_articles(&state, task_id, &body.article_ids).await?;

    // Must happen before the run is enqueued: the orchestrator assembles the
    // prompt from whatever is bound at the time it picks the run up.
    attachments::claim(
        &state.db,
        &body.attachment_ids,
        body.project_id,
        attachments::Owner::Task(task_id),
    )
    .await?;

    let run_id = if body.start {
        // Same gate as `start`: a card created with start=true must not slip
        // past the capability check.
        vet_task(&state, task_id).await?;
        let id = state
            .orchestrator
            .enqueue_task(task_id)
            .await
            .map_err(internal)?;
        sqlx::query("UPDATE tasks SET board_column='running' WHERE id=$1")
            .bind(task_id)
            .execute(&state.db.pool)
            .await
            .map_err(internal)?;
        Some(id)
    } else {
        None
    };
    Ok(Json(json!({ "id": task_id, "runId": run_id })))
}

async fn start(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    vet_task(&state, id).await?;
    let run_id = state.orchestrator.enqueue_task(id).await.map_err(internal)?;
    sqlx::query("UPDATE tasks SET board_column='running' WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "runId": run_id })))
}

/// Refuse to queue a card its engine cannot honour, or that is already being
/// worked on as part of an epic.
///
/// The same check runs again at dispatch, but doing it here means the user
/// sees the reason on the click that caused it rather than as a failed run.
async fn vet_task(state: &AppState, task_id: Uuid) -> Result<(), ApiError> {
    if step_is_live(state, task_id).await? {
        return Err((
            StatusCode::CONFLICT,
            "a teammate is already working on this sub-task as part of its epic".into(),
        ));
    }
    let row = sqlx::query(
        "SELECT COALESCE(a.engine, t.engine) AS engine,
                COALESCE(a.permission_preset, t.permission_mode) AS mode
         FROM tasks t LEFT JOIN agents a ON a.id = t.agent_id WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such task".to_string()))?;

    let mode = match row.get::<Option<String>, _>("mode") {
        Some(m) => serde_json::from_value(Value::String(m)).unwrap_or_default(),
        None => state.orchestrator.default_permission_mode().await,
    };
    match state
        .orchestrator
        .vet_engine(&row.get::<String, _>("engine"), mode)
    {
        Some(reason) => Err((StatusCode::CONFLICT, reason)),
        None => Ok(()),
    }
}

async fn diff(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT t.worktree_path, p.default_branch FROM tasks t
         JOIN projects p ON p.id = t.project_id WHERE t.id=$1",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let Some(worktree): Option<String> = row.get("worktree_path") else {
        return Ok(Json(json!({ "diff": "" })));
    };
    let base: String = row.get("default_branch");
    let diff = state
        .orchestrator
        .worktrees
        .diff(std::path::Path::new(&worktree), &base)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "diff": diff })))
}

async fn merge(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT t.title, t.worktree_path, t.branch, p.path AS project_path, p.default_branch, p.vcs
         FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id=$1",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    // Say which of the two situations this is: a project with no version
    // control can never have a worktree, and "not yet" would be misleading.
    if row.get::<String, _>("vcs") != "git" {
        return Err((
            StatusCode::BAD_REQUEST,
            "this project has no version control, so its tasks edit the folder \
             directly — there is nothing to merge"
                .into(),
        ));
    }
    let (Some(worktree), Some(branch)): (Option<String>, Option<String>) =
        (row.get("worktree_path"), row.get("branch"))
    else {
        return Err((StatusCode::BAD_REQUEST, "task has no worktree yet".into()));
    };
    let wt = aichip_core::worktrees::manager::Worktree {
        path: worktree.into(),
        branch,
    };
    let title: String = row.get("title");
    state
        .orchestrator
        .worktrees
        .squash_merge(
            std::path::Path::new(&row.get::<String, _>("project_path")),
            &wt,
            &row.get::<String, _>("default_branch"),
            &format!("aichip: {title}"),
        )
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    sqlx::query("UPDATE tasks SET board_column='done' WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "merged": true })))
}

async fn run_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT seq, type, payload, ts, step_id FROM events WHERE run_id=$1 ORDER BY seq ASC",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let events: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "seq": r.get::<i64, _>("seq"),
                "ts": r.get::<chrono::DateTime<chrono::Utc>, _>("ts"),
                // Which step produced it — the only way a multi-agent run can
                // attribute an action to a teammate.
                "stepId": r.get::<Option<Uuid>, _>("step_id"),
                "event": r.get::<Value, _>("payload"),
            })
        })
        .collect();
    Ok(Json(json!({ "events": events })))
}

/// Permission requests live in memory while the engine's MCP call blocks on
/// them, so a dashboard refresh needs to re-fetch anything still pending.
async fn pending_permissions(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Json<Value> {
    let pending: Vec<Value> = state
        .permissions
        .pending_for_run(id)
        .into_iter()
        .map(|(request_id, tool_name, input)| {
            json!({ "requestId": request_id, "toolName": tool_name, "input": input })
        })
        .collect();
    Json(json!({ "pending": pending }))
}

/// Stop a run, whatever state it is in.
///
/// A run that is executing gets its step interrupted and its intent
/// recorded, so a multi-step workflow or organization stops rather than
/// rolling on to the next assignment. A run that is merely queued, or
/// parked waiting for plan approval, has no process to interrupt — it is
/// taken off the queue and closed out here instead. This used to answer
/// `{"canceled": true}` no matter what, including when it had done nothing.
async fn cancel_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let status: String = sqlx::query("SELECT status FROM runs WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such run".to_string()))?
        .get("status");

    if matches!(status.as_str(), "completed" | "failed" | "canceled") {
        return Ok(Json(json!({
            "canceled": false,
            "status": status,
            "detail": format!("this run already {status}"),
        })));
    }

    let interrupted = state.orchestrator.cancel(id);

    // Nothing was executing: close it out directly, or the run would sit
    // "queued" forever with a cancel nobody ever reads.
    if !interrupted {
        sqlx::query("DELETE FROM queue WHERE run_id=$1")
            .bind(id)
            .execute(&state.db.pool)
            .await
            .map_err(internal)?;
        sqlx::query(
            "UPDATE runs SET status='canceled', finished_at=now()
             WHERE id=$1 AND status NOT IN ('completed','failed','canceled')",
        )
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
        sqlx::query(
            "UPDATE steps SET status='skipped', finished_at=now()
             WHERE run_id=$1 AND status IN ('queued','running')",
        )
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    }

    Ok(Json(json!({
        "canceled": true,
        "wasRunning": interrupted,
        "detail": if interrupted {
            "stopping — the current step is being interrupted"
        } else {
            "canceled before it started"
        },
    })))
}

#[derive(Deserialize)]
struct Resolve {
    allowed: bool,
}

async fn resolve_permission(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
    Json(body): Json<Resolve>,
) -> Result<Json<Value>, ApiError> {
    if state.permissions.resolve(&request_id, body.allowed) {
        Ok(Json(json!({ "resolved": true })))
    } else {
        Err((StatusCode::NOT_FOUND, "no such pending permission".into()))
    }
}

// ---------------------------------------------------------------------------
// Kanban: card movement, the comment thread, attaching files after creation.

#[derive(Deserialize, Debug)]
struct MoveTask {
    board_column: Option<String>,
    position: Option<f64>,
    /// Who should do this card. Three distinct requests, which is why this is
    /// a nested option: the field absent means "leave the assignee alone",
    /// an explicit `null` means "unassign", and an id means "reassign". A
    /// plain `Option` collapses the first two, so `{"board_column":"done"}`
    /// would silently unassign the card.
    #[serde(default, deserialize_with = "present")]
    agent_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "present")]
    team_id: Option<Option<Uuid>>,
    /// Which CLI runs this card. Absent leaves it alone; a card always has
    /// one, so unlike the assignee there is no "clear it" case.
    #[serde(default)]
    engine: Option<String>,
    /// Absent leaves it alone.
    #[serde(default)]
    plan_first: Option<bool>,
}

/// Distinguish "field was present and null" from "field was absent".
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Drag a card. Dropping a backlog card into "running" is the drag-native way
/// to start it; every other move is bookkeeping. A card whose run is still
/// active refuses to leave "running" — cancel the run first.
async fn move_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<MoveTask>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT t.board_column, t.parent_id,
                (SELECT status FROM runs WHERE task_id = t.id
                 ORDER BY created_at DESC LIMIT 1) AS run_status
         FROM tasks t WHERE t.id=$1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such task".to_string()))?;
    let current: String = row.get("board_column");
    // Two ways a card can be busy, and a sub-ticket is only ever the second.
    // Its work happens under a step in the *epic's* run, so it has no run of its
    // own and `run_status` says nothing about it — which used to leave every
    // guard below wide open for exactly the cards the system is writing to.
    let run_active = matches!(
        row.get::<Option<String>, _>("run_status").as_deref(),
        Some("queued" | "starting" | "running" | "waiting_permission" | "rate_limited")
    ) || step_is_live(&state, id).await?;

    if let Some(column) = &body.board_column {
        if !["backlog", "running", "review", "done"].contains(&column.as_str()) {
            return Err((StatusCode::BAD_REQUEST, format!("unknown column {column}")));
        }
        if run_active && column != "running" {
            return Err((
                StatusCode::CONFLICT,
                "the agent is still working on this card — cancel the run first".into(),
            ));
        }
    }

    // Changing hands mid-run would leave the running agent finishing work the
    // card no longer says is theirs, and the next run would start from a
    // different agent's memory. Cancel first.
    let reassigning = body.agent_id.is_some() || body.team_id.is_some();
    if reassigning && run_active {
        return Err((
            StatusCode::CONFLICT,
            "this card is being worked on — cancel the run before reassigning it".into(),
        ));
    }

    // One level of hierarchy, deliberately. A sub-ticket handed to a team would
    // become an epic of its own: its grandchildren's worktrees would nest inside
    // its parent's, the epic's progress count would double-count it, and a
    // single goal could fan out without bound.
    // `.flatten()`, because the field is a nested option: an explicit null means
    // "take the team off", which is never the thing to refuse.
    if body.team_id.flatten().is_some() && row.get::<Option<Uuid>, _>("parent_id").is_some() {
        return Err((
            StatusCode::CONFLICT,
            "a sub-task can't be handed to a team — split the epic instead".into(),
        ));
    }

    // An agent and a team are alternatives, not a pair: `enqueue_task` hands
    // the whole card to the team when one is set, so leaving both would mean
    // the agent shown on the card never runs.
    let (agent_id, team_id) = match (body.agent_id, body.team_id) {
        (Some(Some(agent)), _) => (Some(Some(agent)), Some(None)),
        (_, Some(Some(team))) => (Some(None), Some(Some(team))),
        other => other,
    };

    if let Some(Some(agent_id)) = agent_id {
        require_same_workspace(&state, id, "agents", agent_id).await?;
    }
    if let Some(Some(team_id)) = team_id {
        require_same_workspace(&state, id, "teams", team_id).await?;
    }

    sqlx::query(
        "UPDATE tasks SET board_column = coalesce($2, board_column),
                          position = coalesce($3, position),
                          agent_id = CASE WHEN $4 THEN $5 ELSE agent_id END,
                          team_id  = CASE WHEN $6 THEN $7 ELSE team_id  END,
                          engine   = coalesce($8, engine),
                          plan_first = coalesce($9, plan_first)
         WHERE id = $1",
    )
    .bind(id)
    .bind(&body.board_column)
    .bind(body.position)
    .bind(agent_id.is_some())
    .bind(agent_id.flatten())
    .bind(team_id.is_some())
    .bind(team_id.flatten())
    .bind(body.engine.as_deref().filter(|e| !e.is_empty()))
    .bind(body.plan_first)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;

    // Dropping into "running" from backlog means "go": start a run unless one
    // is already active or the task already did its work.
    let mut run_id: Option<Uuid> = None;
    if body.board_column.as_deref() == Some("running") && current == "backlog" && !run_active {
        run_id = Some(state.orchestrator.enqueue_task(id).await.map_err(internal)?);
    }
    Ok(Json(json!({ "moved": true, "runId": run_id })))
}

/// Refuse an assignee from another workspace.
///
/// Workspaces are the boundary the rest of the app is built around — the
/// agents list, the mention picker, the team roster are all scoped to one —
/// and a cross-workspace id would produce a card whose assignee is invisible
/// everywhere it should appear.
async fn require_same_workspace(
    state: &AppState,
    task_id: Uuid,
    table: &str,
    assignee_id: Uuid,
) -> Result<(), ApiError> {
    // `table` is a literal from the call sites, never user input.
    let ok: Option<i32> = sqlx::query_scalar(&format!(
        "SELECT 1 FROM {table} x
         JOIN projects p ON p.workspace_id = x.workspace_id
         JOIN tasks t ON t.project_id = p.id
         WHERE t.id = $1 AND x.id = $2"
    ))
    .bind(task_id)
    .bind(assignee_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;

    ok.map(|_| ()).ok_or((
        StatusCode::BAD_REQUEST,
        format!("that {} is not in this card's workspace", table.trim_end_matches('s')),
    ))
}

/// Which agents does a comment speak to? An agent is mentioned when
/// `@its-name` appears, case-insensitively; names may contain spaces, so this
/// is a per-agent substring check rather than token parsing.
fn mentioned_agents(content: &str, agents: &[(Uuid, String)]) -> Vec<Uuid> {
    let lower = content.to_lowercase();
    agents
        .iter()
        .filter(|(_, name)| {
            !name.is_empty() && lower.contains(&format!("@{}", name.to_lowercase()))
        })
        .map(|(id, _)| *id)
        .collect()
}

async fn comments(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT c.id, c.author, c.agent_id, c.content, c.run_id, c.created_at,
                c.file_path, c.line, c.hunk,
                a.name AS agent_name, a.color AS agent_color,
                r.status AS run_status
         FROM task_comments c
         LEFT JOIN agents a ON a.id = c.agent_id
         LEFT JOIN runs r ON r.id = c.run_id
         WHERE c.task_id=$1 ORDER BY c.created_at",
    )
    .bind(task_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    // Replies still being written show as typing indicators, not comments.
    let pending: i64 = sqlx::query(
        "SELECT count(*) AS n FROM runs
         WHERE comment_id IN (SELECT id FROM task_comments WHERE task_id=$1)
           AND status NOT IN ('completed','failed','canceled')",
    )
    .bind(task_id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?
    .get("n");

    Ok(Json(json!({
        "comments": rows.iter().map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "author": r.get::<String, _>("author"),
            "agentId": r.get::<Option<Uuid>, _>("agent_id"),
            "agentName": r.get::<Option<String>, _>("agent_name"),
            "agentColor": r.get::<Option<String>, _>("agent_color"),
            "content": r.get::<String, _>("content"),
            "runId": r.get::<Option<Uuid>, _>("run_id"),
            "filePath": r.get::<Option<String>, _>("file_path"),
            "line": r.get::<Option<i32>, _>("line"),
            "hunk": r.get::<Option<String>, _>("hunk"),
            "ts": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        })).collect::<Vec<_>>(),
        "pendingReplies": pending,
    })))
}

#[derive(Deserialize)]
struct PostComment {
    content: String,
    /// Engine id from `/api/engines`. Omitted means the machine default.
    engine: Option<String>,
    /// Anchor to a line of the diff. Present when the comment was written
    /// from the diff view rather than the card.
    file_path: Option<String>,
    line: Option<i32>,
    /// The hunk as it looked when the note was written — snapshotted because
    /// the fix run changes the very diff the line number refers to.
    hunk: Option<String>,
    /// Act on it, rather than just record it. Spawns a scoped run in the
    /// task's existing worktree.
    fix: Option<bool>,
    /// Knowledge-base articles referenced by this comment alone. Scoped to the
    /// comment rather than pinned to the card: "see #runbook" is context for
    /// this reply, not a permanent property of the work.
    #[serde(default)]
    article_ids: Vec<Uuid>,
}

async fn post_comment(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<PostComment>,
) -> Result<Json<Value>, ApiError> {
    let content = body.content.trim();
    if content.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "comment is empty".into()));
    }
    // The task's workspace bounds who can be mentioned.
    let agents: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT a.id, a.name FROM agents a
         JOIN projects p ON p.workspace_id = a.workspace_id
         JOIN tasks t ON t.project_id = p.id
         WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    if agents.is_empty() {
        // Distinguish "no such task" from "no agents to mention".
        let exists = sqlx::query("SELECT 1 AS ok FROM tasks WHERE id=$1")
            .bind(task_id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?;
        if exists.is_none() {
            return Err((StatusCode::NOT_FOUND, "no such task".into()));
        }
    }

    let comment_id: Uuid = sqlx::query(
        "INSERT INTO task_comments (task_id, author, content, file_path, line, hunk)
         VALUES ($1,'user',$2,$3,$4,$5) RETURNING id",
    )
    .bind(task_id)
    .bind(content)
    .bind(body.file_path.as_deref().map(str::trim).filter(|p| !p.is_empty()))
    .bind(body.line)
    .bind(body.hunk.as_deref())
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?
    .get("id");

    if !body.article_ids.is_empty() {
        sqlx::query(
            "INSERT INTO comment_articles (comment_id, article_id)
             SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING",
        )
        .bind(comment_id)
        .bind(&body.article_ids)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    }

    // "Fix this" is its own path: a comment reply is read-only by design, so
    // acting on review feedback needs a run that can actually edit, in the
    // worktree the diff came from.
    if body.fix.unwrap_or(false) {
        let run_id = state
            .orchestrator
            .enqueue_review_fix(comment_id)
            .await
            .map_err(internal)?;
        return Ok(Json(json!({ "id": comment_id, "runIds": [run_id], "fixRunId": run_id })));
    }

    // Every mentioned agent replies, capped so one comment can't fan out a
    // whole roster of runs.
    let default_engine = state.orchestrator.default_engine();
    let engine = body.engine.as_deref().unwrap_or(&default_engine);
    let mut run_ids: Vec<Uuid> = vec![];
    for agent_id in mentioned_agents(content, &agents).into_iter().take(3) {
        run_ids.push(
            state
                .orchestrator
                .enqueue_comment_reply(comment_id, agent_id, engine)
                .await
                .map_err(internal)?,
        );
    }
    Ok(Json(json!({ "id": comment_id, "runIds": run_ids })))
}

#[derive(Deserialize)]
struct BakeoffBody {
    variants: Vec<VariantBody>,
}

#[derive(Deserialize)]
struct VariantBody {
    label: String,
    agent_id: Option<Uuid>,
    /// "easy" | "medium" | "complex". Absent means the agent's own tier.
    tier: Option<String>,
    /// Absent means the card's engine.
    engine: Option<String>,
}

/// Run the same brief several ways at once.
async fn start_bakeoff(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<BakeoffBody>,
) -> Result<Json<Value>, ApiError> {
    let variants: Vec<Variant> = body
        .variants
        .into_iter()
        .map(|v| Variant {
            label: v.label,
            agent_id: v.agent_id,
            tier: v.tier,
            engine: v.engine,
        })
        .collect();

    let run_ids = state
        .orchestrator
        .enqueue_bakeoff(task_id, &variants)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "runIds": run_ids })))
}

/// The variants of a task, with their diffs, so they can be read side by side.
///
/// The diff is the comparison — cost and duration matter, but nobody picks a
/// winner on a number. Each is fetched from that variant's own worktree.
async fn bakeoff(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT r.id, r.variant_label, r.status, r.cost_usd, r.model, r.worktree_path, r.engine,
                r.started_at, r.finished_at, r.error_reason,
                a.name AS agent_name, p.default_branch
         FROM runs r
         JOIN tasks t ON t.id = r.task_id
         JOIN projects p ON p.id = t.project_id
         LEFT JOIN agents a ON a.id = r.agent_id
         WHERE r.task_id = $1 AND r.variant_label IS NOT NULL
         ORDER BY r.created_at",
    )
    .bind(task_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let mut variants = vec![];
    for r in &rows {
        let diff = match r.get::<Option<String>, _>("worktree_path") {
            Some(path) => state
                .orchestrator
                .worktrees
                .diff(std::path::Path::new(&path), &r.get::<String, _>("default_branch"))
                .await
                .unwrap_or_default(),
            None => String::new(),
        };
        let started = r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("started_at");
        let finished = r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("finished_at");
        variants.push(json!({
            "runId": r.get::<Uuid, _>("id"),
            "label": r.get::<String, _>("variant_label"),
            "status": r.get::<String, _>("status"),
            "agentName": r.get::<Option<String>, _>("agent_name"),
            "model": r.get::<Option<String>, _>("model"),
            "engine": r.get::<String, _>("engine"),
            "costUsd": r.get::<Option<f64>, _>("cost_usd"),
            "error": r.get::<Option<String>, _>("error_reason"),
            "seconds": match (started, finished) {
                (Some(a), Some(b)) => Some((b - a).num_seconds()),
                _ => None,
            },
            // Cheap, comparable signal to sit beside the diff itself.
            "linesChanged": diff
                .lines()
                .filter(|l| (l.starts_with('+') || l.starts_with('-'))
                    && !l.starts_with("+++") && !l.starts_with("---"))
                .count(),
            "diff": diff,
        }));
    }
    Ok(Json(json!({ "variants": variants })))
}

/// Adopt a variant's work as the task's and discard the rest.
async fn keep_variant(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    state
        .orchestrator
        .keep_variant(run_id)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "kept": run_id })))
}

#[derive(Deserialize)]
struct AttachToTask {
    attachment_ids: Vec<Uuid>,
}

/// Bind already-uploaded files to an existing card — the drawer's attach
/// button. The next run of the task will see them.
async fn attach_to_task(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<AttachToTask>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query("SELECT project_id FROM tasks WHERE id=$1")
        .bind(task_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such task".to_string()))?;
    attachments::claim(
        &state.db,
        &body.attachment_ids,
        row.get("project_id"),
        attachments::Owner::Task(task_id),
    )
    .await?;
    Ok(Json(json!({ "attached": body.attachment_ids.len() })))
}

#[cfg(test)]
mod tests {
    use super::{mentioned_agents, MoveTask};
    use uuid::Uuid;

    /// The distinction the whole reassignment feature rests on. If an absent
    /// field deserialized the same as an explicit null, then dragging a card
    /// between columns — which sends only `board_column` — would quietly
    /// unassign it.
    #[test]
    fn an_absent_assignee_is_not_the_same_as_a_null_one() {
        let drag: MoveTask = serde_json::from_str(r#"{"board_column":"done"}"#).unwrap();
        assert_eq!(drag.agent_id, None, "absent must mean leave it alone");
        assert_eq!(drag.team_id, None);

        let unassign: MoveTask = serde_json::from_str(r#"{"agent_id":null}"#).unwrap();
        assert_eq!(unassign.agent_id, Some(None), "null must mean clear it");

        let id = Uuid::new_v4();
        let reassign: MoveTask =
            serde_json::from_str(&format!(r#"{{"agent_id":"{id}"}}"#)).unwrap();
        assert_eq!(reassign.agent_id, Some(Some(id)));
    }

    /// The client always sends both ids, so "give it to this team" arrives as
    /// a team id plus a null agent — and must not be read as ambiguous.
    #[test]
    fn handing_a_card_to_a_team_clears_the_agent_in_the_same_request() {
        let team = Uuid::new_v4();
        let body: MoveTask =
            serde_json::from_str(&format!(r#"{{"agent_id":null,"team_id":"{team}"}}"#)).unwrap();
        assert_eq!(body.agent_id, Some(None));
        assert_eq!(body.team_id, Some(Some(team)));
    }

    #[test]
    fn an_empty_patch_touches_nothing() {
        let body: MoveTask = serde_json::from_str("{}").unwrap();
        assert!(body.board_column.is_none());
        assert!(body.position.is_none());
        assert_eq!(body.agent_id, None);
        assert_eq!(body.team_id, None);
    }

    #[test]
    fn mentions_match_case_insensitively_and_allow_spaces_in_names() {
        let rex = Uuid::new_v4();
        let ada = Uuid::new_v4();
        let agents = vec![
            (rex, "Rex".to_string()),
            (ada, "Ada Lovelace".to_string()),
        ];
        assert_eq!(mentioned_agents("hey @rex, look at this", &agents), vec![rex]);
        assert_eq!(mentioned_agents("@Ada Lovelace what do you think?", &agents), vec![ada]);
        assert_eq!(
            mentioned_agents("@rex and @ada lovelace both", &agents),
            vec![rex, ada]
        );
        assert!(mentioned_agents("mail me at rex@example.com", &agents).is_empty());
        assert!(mentioned_agents("no mentions here", &agents).is_empty());
    }
}

// ---------------------------------------------------------------------------
// Deleting and retrying a card.

/// True while an org run owns this card through a live assignment.
///
/// The companion to `run_is_active`, and necessary because a sub-ticket has no
/// run of its own: an epic's work happens under steps of the *epic's* run. Every
/// "is this card busy" check needs both, or the one class of card the system
/// writes to is the one class nothing protects.
async fn step_is_live(state: &AppState, task_id: Uuid) -> Result<bool, ApiError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM steps s JOIN runs r ON r.id = s.run_id
              WHERE s.task_id = $1
                AND s.status IN ('queued','starting','running','waiting_permission','rate_limited')
                AND r.status NOT IN ('completed','failed','canceled'))",
    )
    .bind(task_id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)
}

/// True when the task's latest run is still live.
async fn run_is_active(state: &AppState, task_id: Uuid) -> Result<bool, ApiError> {
    let row = sqlx::query(
        "SELECT status FROM runs WHERE task_id=$1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(matches!(
        row.map(|r| r.get::<String, _>("status")).as_deref(),
        Some("queued" | "starting" | "running" | "waiting_permission" | "rate_limited")
    ))
}

/// Drop a task's worktree and its branch, and forget them on the row.
///
/// This is the only production caller of `WorktreeManager::remove` — without
/// it, every task ever run leaves a worktree and an `aichip/*` branch behind
/// forever.
async fn drop_worktree(state: &AppState, task_id: Uuid) -> Result<(), ApiError> {
    let row = sqlx::query(
        "SELECT t.worktree_path, t.branch, p.path AS project_path
         FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id=$1",
    )
    .bind(task_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such task".to_string()))?;

    if let (Some(path), Some(branch)) = (
        row.get::<Option<String>, _>("worktree_path"),
        row.get::<Option<String>, _>("branch"),
    ) {
        // An epic and its sub-tickets share one checkout, so removing it because
        // one of them is being deleted would pull the ground out from under the
        // others. Forget it on this row and leave the directory alone.
        let shared: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tasks WHERE worktree_path = $1 AND id <> $2)",
        )
        .bind(&path)
        .bind(task_id)
        .fetch_one(&state.db.pool)
        .await
        .map_err(internal)?;
        if shared {
            sqlx::query("UPDATE tasks SET worktree_path=NULL, branch=NULL WHERE id=$1")
                .bind(task_id)
                .execute(&state.db.pool)
                .await
                .map_err(internal)?;
            return Ok(());
        }

        let wt = aichip_core::worktrees::manager::Worktree { path: path.into(), branch };
        // Best effort: a worktree the user already deleted by hand must not
        // block deleting the card.
        if let Err(e) = state
            .orchestrator
            .worktrees
            .remove(std::path::Path::new(&row.get::<String, _>("project_path")), &wt)
            .await
        {
            tracing::warn!(%task_id, error = %e, "could not remove worktree");
        }
        sqlx::query("UPDATE tasks SET worktree_path=NULL, branch=NULL WHERE id=$1")
            .bind(task_id)
            .execute(&state.db.pool)
            .await
            .map_err(internal)?;
    }
    Ok(())
}

/// Delete a card: its comments, runs, and attachment rows go with it (FK
/// cascade), its worktree and branch are removed, and the attachment bytes are
/// reclaimed by the sweeper. The agent's memory of the work survives —
/// `agent_memories.task_id` is SET NULL, because what an agent learned
/// shouldn't vanish when a card is tidied away.
async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if run_is_active(&state, id).await? || step_is_live(&state, id).await? {
        return Err((
            StatusCode::CONFLICT,
            "the agent is still working on this card — cancel the run first".into(),
        ));
    }
    // Sub-tickets share the epic's checkout, so they must stop pointing at it
    // before it is removed. Without this, deleting an epic leaves a column of
    // cards whose "diff" is a directory that no longer exists.
    //
    // The rows themselves survive: `parent_id` is ON DELETE SET NULL, because a
    // sub-ticket is real work with its own comments and history, and tidying the
    // epic away should not take it with them.
    sqlx::query("UPDATE tasks SET worktree_path=NULL, branch=NULL WHERE parent_id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    drop_worktree(&state, id).await?;
    let done = sqlx::query("DELETE FROM tasks WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    if done.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "no such task".into()));
    }
    Ok(Json(json!({ "deleted": true })))
}

#[derive(Deserialize)]
struct Retry {
    /// Start from a clean checkout (default). False continues in the existing
    /// worktree, keeping whatever the previous attempt left behind.
    #[serde(default = "yes")]
    fresh: bool,
}

fn yes() -> bool {
    true
}

/// Run a card again.
///
/// A fresh retry throws away the previous attempt's worktree and branch, so
/// the agent starts from the base branch rather than silently inheriting its
/// own half-finished work. That discards an unmerged diff, which is the point
/// of retrying — but it is destructive, so the UI confirms it for cards
/// sitting in review.
async fn retry(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Option<Json<Retry>>,
) -> Result<Json<Value>, ApiError> {
    if run_is_active(&state, id).await? || step_is_live(&state, id).await? {
        return Err((
            StatusCode::CONFLICT,
            "this card is already running — cancel it before retrying".into(),
        ));
    }
    let fresh = body.map(|Json(b)| b.fresh).unwrap_or(true);
    if fresh {
        drop_worktree(&state, id).await?;
    }
    let run_id = state.orchestrator.enqueue_task(id).await.map_err(internal)?;
    sqlx::query("UPDATE tasks SET board_column='running' WHERE id=$1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "runId": run_id, "fresh": fresh })))
}

// ── Plan-first cards ────────────────────────────────────────────────────────
//
// A card can ask the agent to write down what it means to do before it does
// it. The run parks at `awaiting_approval` with the plan stored as a step, and
// these routes are how a person confirms it, rewrites it, or sends it back.
//
// Everything here refuses on a run that isn't parked. A plan being edited
// while work is already underway would describe a decision nobody gets to
// make, which is worse than no plan at all.

/// Only a parked run's plan is editable.
async fn assert_parked(state: &AppState, run_id: Uuid) -> Result<(), ApiError> {
    let status: String = sqlx::query_scalar("SELECT status FROM runs WHERE id=$1")
        .bind(run_id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such run".to_string()))?;
    if status != "awaiting_approval" {
        return Err((
            StatusCode::CONFLICT,
            format!("this run is {status}, so its plan is no longer editable"),
        ));
    }
    Ok(())
}

async fn plan(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT r.status, r.plan_edited, s.output_text, s.finished_at
         FROM runs r
         LEFT JOIN steps s ON s.run_id = r.id AND s.step_key = 'plan'
         WHERE r.id = $1",
    )
    .bind(run_id)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such run".to_string()))?;

    let status: String = row.get("status");
    Ok(Json(json!({
        "runId": run_id,
        "content": row.get::<Option<String>, _>("output_text"),
        // Only while parked can it be answered; afterwards it's a record of
        // what was agreed, which is still worth showing.
        "awaitingApproval": status == "awaiting_approval",
        "edited": row.get::<bool, _>("plan_edited"),
        "writtenAt": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("finished_at"),
    })))
}

#[derive(Deserialize)]
struct PlanEdit {
    content: String,
}

/// Rewrite the plan by hand. The work pass is told the text was edited, so it
/// follows what's in front of it rather than what it remembers proposing.
async fn edit_plan(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(body): Json<PlanEdit>,
) -> Result<Json<Value>, ApiError> {
    assert_parked(&state, run_id).await?;
    if body.content.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "an empty plan approves nothing — delete the card instead".into(),
        ));
    }
    let updated = sqlx::query(
        "UPDATE steps SET output_text = $2 WHERE run_id = $1 AND step_key = 'plan'",
    )
    .bind(run_id)
    .bind(body.content.trim())
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    if updated.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "this run has no plan".into()));
    }
    sqlx::query("UPDATE runs SET plan_edited = TRUE WHERE id = $1")
        .bind(run_id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "saved": true })))
}

/// Start the work, from whatever the plan says now.
async fn approve_plan(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let updated = sqlx::query(
        "UPDATE runs SET plan_approved_at = now(), status = 'queued'
         WHERE id = $1 AND status = 'awaiting_approval'",
    )
    .bind(run_id)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            "this run is not waiting for approval".into(),
        ));
    }
    // Re-queued rather than resumed in place: the planning dispatch already
    // released its slot, so this takes a fresh one when the queue has room.
    state.orchestrator.queue(run_id, 10).await.map_err(internal)?;
    Ok(Json(json!({ "approved": true })))
}

#[derive(Deserialize)]
struct Revise {
    note: String,
}

/// Send the plan back for another pass, saying what was wrong with it.
async fn revise_plan(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Json(body): Json<Revise>,
) -> Result<Json<Value>, ApiError> {
    if body.note.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "say what to change — a rejection with no reason just burns another pass".into(),
        ));
    }
    // The rejected plan stays on file: the next pass is shown it alongside the
    // feedback, so it can answer the objection rather than start from nothing.
    // A written-but-unapproved plan re-plans rather than runs, which is what
    // makes leaving the row safe.
    //
    // The status check is in the WHERE clause rather than a separate read
    // beforehand. Checking and then writing unconditionally leaves a window in
    // which the run is approved and dispatched between the two, and this would
    // then re-queue a run that was already working.
    let updated = sqlx::query(
        "UPDATE runs SET plan_note = $2, plan_edited = FALSE, status = 'queued'
         WHERE id = $1 AND status = 'awaiting_approval'",
    )
    .bind(run_id)
    .bind(body.note.trim())
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    if updated.rows_affected() == 0 {
        return Err((
            StatusCode::CONFLICT,
            "this run is not waiting for approval".into(),
        ));
    }
    state.orchestrator.queue(run_id, 10).await.map_err(internal)?;
    Ok(Json(json!({ "revising": true })))
}

// ── Knowledge-base articles on a card ───────────────────────────────────────

async fn link_articles(state: &AppState, task_id: Uuid, ids: &[Uuid]) -> Result<(), ApiError> {
    // A tagged page's full text is injected into the run's prompt, so tagging
    // is a read grant. Without this, a page from another workspace could be
    // attached to this card and handed to an agent working in it.
    for id in ids {
        require_same_workspace(state, task_id, "kb_articles", *id).await?;
    }
    sqlx::query("DELETE FROM task_articles WHERE task_id = $1 AND NOT (article_id = ANY($2))")
        .bind(task_id)
        .bind(ids)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO task_articles (task_id, article_id)
         SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING",
    )
    .bind(task_id)
    .bind(ids)
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(())
}

async fn task_articles(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT a.id, a.title, a.summary, a.status, a.origin
         FROM task_articles ta JOIN kb_articles a ON a.id = ta.article_id
         WHERE ta.task_id = $1 ORDER BY a.title",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "articles": rows.iter().map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "title": r.get::<String, _>("title"),
            "summary": r.get::<String, _>("summary"),
            "status": r.get::<String, _>("status"),
            "origin": r.get::<String, _>("origin"),
        })).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
struct ArticleLinks {
    article_ids: Vec<Uuid>,
}

/// Replace the set of articles tagged onto a card.
///
/// A full replacement rather than add/remove endpoints: the UI holds the whole
/// list anyway, and two endpoints invite the state where the client and the
/// server disagree about what is attached.
async fn set_task_articles(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ArticleLinks>,
) -> Result<Json<Value>, ApiError> {
    link_articles(&state, id, &body.article_ids).await?;
    Ok(Json(json!({ "linked": body.article_ids.len() })))
}
