use super::{internal, ApiError};
use crate::AppState;
use aichip_core::worktrees::manager::{self, ensure_repo_state, Vcs};
use aichip_shared::{ReasoningEffort, TierChoice};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects", get(list).post(create))
        .route("/projects/{id}", get(one).patch(update).delete(unload))
        .route("/projects/{id}/worktrees", get(worktrees))
        .route("/projects/{id}/worktrees/reclaim", post(reclaim_worktrees))
        .route("/projects/{id}/checkout", get(checkout))
        .route("/projects/{id}/checkout/stash", post(stash_checkout))
        .route("/projects/{id}/checkout/commit", post(commit_checkout))
}

const PROJECT_COLUMNS: &str = "id, path, name, default_branch, workspace_id, vcs, vcs_note, \
     full_auto_opt_in, kind, github_repo, default_engine, default_tier, default_effort";

fn project_json(r: &sqlx::postgres::PgRow) -> Value {
    json!({
        "id": r.get::<Uuid, _>("id"),
        "path": r.get::<String, _>("path"),
        "name": r.get::<String, _>("name"),
        "defaultBranch": r.get::<String, _>("default_branch"),
        "workspaceId": r.get::<Uuid, _>("workspace_id"),
        "vcs": r.get::<String, _>("vcs"),
        "vcsNote": r.get::<Option<String>, _>("vcs_note"),
        "fullAutoOptIn": r.get::<bool, _>("full_auto_opt_in"),
        "kind": r.get::<String, _>("kind"),
        // Resolved lazily on first use and cached, so this is NULL until some
        // GitHub feature has asked. Absent means "not asked yet", never "not a
        // GitHub project".
        "githubRepo": r.get::<Option<String>, _>("github_repo"),
        // Null means inherit, everywhere. A project that pins nothing behaves
        // exactly as it did before these existed.
        "defaultEngine": r.get::<Option<String>, _>("default_engine"),
        "defaultTier": r.get::<Option<String>, _>("default_tier"),
        "defaultEffort": r.get::<Option<String>, _>("default_effort"),
    })
}

/// One project by id, whatever kind it is.
///
/// Deliberately without the `kind = 'repo'` filter the list carries. An app is
/// a project, and its folder is the only place its source lives — so the files
/// editor has to be able to reach it. Filtered here as well would mean a page
/// that renders with a header reading "Project" and every setting silently
/// defaulted, which is what happened while this route did not exist.
async fn one(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(&format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = $1"))
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;

    // The point of use for the repository's identity: this is what the project
    // page fetches, so a GitHub project learns its own name the first time
    // somebody opens it, and every surface after that reads a column. Resolving
    // is a `git remote get-url` at most once per project, and never for a
    // project that has no remote to read.
    let mut body = project_json(&row);
    if row.get::<Option<String>, _>("github_repo").is_none() {
        if let Some(slug) = aichip_core::github::repo::resolve(&state.db, id).await {
            body["githubRepo"] = Value::String(slug);
        }
    }
    Ok(Json(body))
}

/// Take a project out of aichip. Its folder stays exactly where it is.
///
/// "Remove" next to a filesystem path reads as "delete my code", so this is
/// called unload everywhere it is offered and the confirm says outright that
/// nothing on disk is touched. What goes is aichip's own record — the cards,
/// their runs and comments, the chats — plus the worktrees and `aichip/*`
/// branches aichip created, which is the one thing it *would* leave behind.
///
/// Until this existed, the only way to remove a project was to delete its whole
/// workspace, which has no UI either. Load the wrong folder once and it was in
/// the sidebar forever.
async fn unload(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    // Same shape as deleting a card: a live agent owns its worktree, and
    // pulling the project row out from under a running process leaves it
    // writing into a directory nothing will ever look at.
    let live: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM runs r JOIN tasks t ON t.id = r.task_id
             WHERE t.project_id = $1 AND r.status NOT IN ('completed','failed','canceled'))",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    if live {
        return Err((
            StatusCode::CONFLICT,
            "an agent is still working in this project — cancel its run first".into(),
        ));
    }

    // Before the row goes: afterwards nothing knows the repository's path, and
    // a worktree whose repo cannot be named can never be removed by git again.
    // Best-effort, and the unload proceeds either way — a folder left behind is
    // recoverable, a project you cannot remove is not.
    if let Ok(held) = held_for(&state, id).await {
        let (path, _) = git_project(&state, id).await?;
        let repo = std::path::PathBuf::from(&path);
        for h in &held {
            let wt = aichip_core::worktrees::manager::Worktree {
                path: h.path.clone(),
                branch: h.branch.clone(),
            };
            if let Err(e) = state.orchestrator.worktrees.remove(&repo, &wt).await {
                tracing::warn!(branch = %h.branch, error = %e, "could not remove worktree while unloading");
            }
        }
    }

    // Containers outlive rows too.
    if let Err(e) = aichip_core::previews::stop_for_project(&state.db, id).await {
        tracing::warn!(project = %id, error = %e, "could not stop previews while unloading");
    }

    let done = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    if done.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, "no such project".into()));
    }
    Ok(Json(json!({ "unloaded": true })))
}

/// What this project's finished cards are still holding on disk.
///
/// Previews got a disk figure and a reclaim link; worktrees are the larger
/// artifact and got neither, so they accumulated silently — 2.9 GB across 22
/// directories on the machine this was written on, every one of them belonging
/// to a card that had already landed.
async fn worktrees(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let held = held_for(&state, id).await?;

    // Which card each directory belongs to, so the list reads as work rather
    // than as paths. A directory with no row is exactly the kind this sweep
    // exists for — a bake-off variant whose run cascaded away, or a workflow
    // fan-out worktree whose id was never written down at all.
    let owners = sqlx::query(
        "SELECT worktree_path, title FROM tasks
         WHERE project_id = $1 AND worktree_path IS NOT NULL",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let rows: Vec<Value> = held
        .iter()
        .map(|h| {
            let title = owners
                .iter()
                .find(|r| r.get::<String, _>("worktree_path") == h.path.to_string_lossy())
                .map(|r| r.get::<String, _>("title"));
            json!({
                "path": h.path,
                "branch": h.branch,
                "bytes": h.bytes,
                "dirty": h.dirty,
                "landed": h.landed,
                "reclaimable": h.reclaimable(),
                "keptBecause": h.kept_because(),
                "title": title,
            })
        })
        .collect();

    Ok(Json(json!({
        "worktrees": rows,
        "bytes": held.iter().map(|h| h.bytes).sum::<u64>(),
        "reclaimable": held.iter().filter(|h| h.reclaimable()).count(),
        "reclaimableBytes": held.iter().filter(|h| h.reclaimable()).map(|h| h.bytes).sum::<u64>(),
    })))
}

/// Release the ones that can be proved safe, and say why each of the rest stays.
///
/// Explicit rather than automatic, for the reason `previews::reclaim_disk` is:
/// disk you can see and choose to free is a different thing from disk something
/// frees behind you.
async fn reclaim_worktrees(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let (path, _) = git_project(&state, id).await?;
    let repo = std::path::PathBuf::from(&path);
    let held = held_for(&state, id).await?;

    // A card whose agent is working right now owns its worktree, whatever git
    // thinks of the branch.
    let busy: Vec<String> = sqlx::query_scalar(
        "SELECT t.worktree_path FROM tasks t
         WHERE t.project_id = $1 AND t.worktree_path IS NOT NULL
           AND EXISTS (SELECT 1 FROM runs r WHERE r.task_id = t.id
                       AND r.status NOT IN ('completed','failed','canceled'))",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let mut freed = 0u64;
    let mut released: Vec<Value> = Vec::new();
    let mut kept: Vec<Value> = Vec::new();
    for h in &held {
        let running = busy.iter().any(|p| *p == h.path.to_string_lossy());
        let why = if running {
            Some("its agent is still working")
        } else {
            h.kept_because()
        };
        if let Some(why) = why {
            kept.push(json!({ "branch": h.branch, "why": why }));
            continue;
        }
        let wt = aichip_core::worktrees::manager::Worktree {
            path: h.path.clone(),
            branch: h.branch.clone(),
        };
        match state.orchestrator.worktrees.remove(&repo, &wt).await {
            Ok(()) => {
                freed += h.bytes;
                released.push(json!({ "branch": h.branch, "bytes": h.bytes }));
            }
            Err(e) => {
                tracing::warn!(branch = %h.branch, error = %e, "could not reclaim worktree");
                kept.push(json!({ "branch": h.branch, "why": "git would not remove it" }));
            }
        }
    }

    // The rows that named what was just removed would otherwise point at
    // nothing, and the Files tab reads them to build its tree picker.
    for r in &released {
        sqlx::query("UPDATE tasks SET worktree_path=NULL, branch=NULL WHERE project_id=$1 AND branch=$2")
            .bind(id)
            .bind(r["branch"].as_str().unwrap_or_default())
            .execute(&state.db.pool)
            .await
            .map_err(internal)?;
    }

    Ok(Json(json!({ "released": released, "kept": kept, "bytes": freed })))
}

async fn held_for(
    state: &AppState,
    id: Uuid,
) -> Result<Vec<aichip_core::worktrees::manager::Held>, ApiError> {
    let row = sqlx::query("SELECT path, vcs, default_branch FROM projects WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    if row.get::<String, _>("vcs") != "git" {
        return Ok(vec![]);
    }
    manager::inventory(
        std::path::Path::new(&row.get::<String, _>("path")),
        &row.get::<String, _>("default_branch"),
    )
    .await
    .map_err(internal)
}

/// The project's own checkout, and what is standing in a merge's way.
///
/// The merge guard has always been able to *name* these files; nothing could
/// read them, so "commit or stash them and merge again" was an instruction you
/// could only follow in a terminal. Read-only, and it runs the same query the
/// guard does, so the two cannot disagree about which files count.
async fn checkout(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let (path, vcs) = git_project(&state, id).await?;
    if !vcs {
        // Not an error: a project that edits in place has no merge to block,
        // so the honest answer is an empty one.
        return Ok(Json(json!({ "vcs": false, "branch": null, "dirty": [] })));
    }
    let (branch, dirty) = manager::checkout_status(std::path::Path::new(&path))
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "vcs": true,
        "path": path,
        "branch": branch,
        "dirty": dirty.iter().map(|f| json!({
            "index": f.index.to_string(),
            "worktree": f.worktree.to_string(),
            "path": f.path,
        })).collect::<Vec<_>>(),
    })))
}

/// Set the checkout's changes aside. Reversible, and the response says how.
async fn stash_checkout(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let path = dirty_git_project(&state, id).await?;
    let repo = std::path::Path::new(&path);
    manager::stash(repo, "aichip: set aside so a card could merge")
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({
        "stashed": true,
        "undo": "git stash pop",
    })))
}

/// Commit the checkout's changes as their own commit, under the person's name.
///
/// Their work, so their commit — never folded into the card's, which is the
/// whole reason the merge refuses in the first place.
async fn commit_checkout(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let path = dirty_git_project(&state, id).await?;
    let repo = std::path::Path::new(&path);
    let wrote = manager::commit_all(repo, "Work in progress")
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({
        "committed": wrote,
        "undo": "git reset --soft HEAD~1",
    })))
}

/// A project that has a repository, or an explanation of why the request makes
/// no sense for it.
async fn git_project(state: &AppState, id: Uuid) -> Result<(String, bool), ApiError> {
    let row = sqlx::query("SELECT path, vcs FROM projects WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    Ok((row.get("path"), row.get::<String, _>("vcs") == "git"))
}

/// The same, for the two routes that change something: they need a repository
/// *and* something to act on. Refusing an empty checkout rather than writing an
/// empty commit or an empty stash keeps a double-click from doing anything.
async fn dirty_git_project(state: &AppState, id: Uuid) -> Result<String, ApiError> {
    let (path, vcs) = git_project(state, id).await?;
    if !vcs {
        return Err((
            StatusCode::BAD_REQUEST,
            "this project has no version control, so there is nothing to commit or stash".into(),
        ));
    }
    let (_, dirty) = manager::checkout_status(std::path::Path::new(&path))
        .await
        .map_err(internal)?;
    if dirty.is_empty() {
        return Err((
            StatusCode::CONFLICT,
            "your checkout has no uncommitted changes — nothing to set aside".into(),
        ));
    }
    Ok(path)
}

#[derive(Deserialize)]
pub struct WorkspaceFilter {
    pub workspace_id: Option<Uuid>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<WorkspaceFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(&format!(
        // Apps are projects too — that is what gives them worktrees, diffs and
        // previews — but they belong in the gallery, not in a list of the
        // repositories someone works in.
        "SELECT {PROJECT_COLUMNS} FROM projects
         WHERE kind = 'repo' AND ($1::uuid IS NULL OR workspace_id = $1)
         ORDER BY created_at DESC"
    ))
    .bind(filter.workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let projects: Vec<Value> = rows.iter().map(project_json).collect();
    Ok(Json(json!({ "projects": projects })))
}

#[derive(Deserialize)]
struct CreateProject {
    workspace_id: Uuid,
    path: String,
    name: Option<String>,
    default_branch: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateProject>,
) -> Result<Json<Value>, ApiError> {
    let path = std::path::Path::new(&body.path);
    if !path.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{} is not a directory", body.path),
        ));
    }
    let default_branch = body.default_branch.unwrap_or_else(|| "main".into());

    // Initialize a repository if this folder needs one. A worktree is what
    // keeps an agent out of the user's files and what makes review possible,
    // so it's worth creating a repo to get one — but a folder that can't have
    // one still becomes a project, with its runs happening in place.
    // Record the branch the repository is actually on. Assuming "main" for a
    // repo that uses something else makes every later `worktree add` fail.
    let repo = ensure_repo_state(path, &default_branch).await;
    let default_branch = repo.branch;
    let (vcs, vcs_note) = match repo.vcs {
        Vcs::Git => ("git", None),
        Vcs::None(reason) => ("none", Some(reason)),
    };

    let name = body.name.unwrap_or_else(|| {
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into())
    });
    let row = sqlx::query(
        "INSERT INTO projects (workspace_id, path, name, default_branch, vcs, vcs_note)
         VALUES ($1,$2,$3,$4,$5,$6)
         ON CONFLICT (path) DO UPDATE SET name = EXCLUDED.name,
             workspace_id = EXCLUDED.workspace_id,
             vcs = EXCLUDED.vcs, vcs_note = EXCLUDED.vcs_note
         RETURNING id",
    )
    .bind(body.workspace_id)
    .bind(&body.path)
    .bind(&name)
    .bind(&default_branch)
    .bind(vcs)
    .bind(&vcs_note)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "id": row.get::<Uuid, _>("id"),
        "name": name,
        "vcs": vcs,
        "vcsNote": vcs_note,
    })))
}

#[derive(Deserialize)]
struct UpdateProject {
    /// Let agents work in this project without stopping to ask.
    ///
    /// This is the switch the FullAuto gate has always read and nothing has
    /// ever been able to set — which is why every run prompted for every
    /// action with no way to turn it off.
    full_auto_opt_in: Option<bool>,
    default_branch: Option<String>,
    /// What to call it. The folder's basename until somebody says otherwise —
    /// two clones of the same repository are both called `cli`, and only the
    /// person looking at them knows which is which.
    name: Option<String>,
    /// What new cards here start on. `Some(None)` clears the pin and goes back
    /// to inheriting; the field being absent leaves it alone. Typed so a client
    /// that invents a tier is a 422 rather than a row the orchestrator has to
    /// shrug at when a run actually starts.
    #[serde(default, deserialize_with = "double_option")]
    default_engine: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    default_tier: Option<Option<TierChoice>>,
    #[serde(default, deserialize_with = "double_option")]
    default_effort: Option<Option<ReasoningEffort>>,
}

/// Tell "absent" apart from "explicitly null".
///
/// `COALESCE` cannot express "clear this", and without the distinction there is
/// no way to un-pin a project once pinned — the same shape `chats` needed for
/// its own inheritable settings.
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(d).map(Some)
}

async fn update(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
    Json(body): Json<UpdateProject>,
) -> Result<Json<Value>, ApiError> {
    // Rejected rather than trimmed to nothing: a project with a blank name is
    // an unclickable row in the sidebar.
    let name = match body.name.as_deref().map(str::trim) {
        Some("") => {
            return Err((StatusCode::BAD_REQUEST, "a project needs a name".into()));
        }
        other => other.map(str::to_string),
    };

    // Working without prompts is only safe because the run happens in an
    // aichip-managed worktree you review before merging. A project with no
    // version control edits your files directly, so there is nothing to
    // review and nothing to roll back — refuse rather than quietly downgrade
    // it later and leave the toggle looking enabled.
    if body.full_auto_opt_in == Some(true) {
        let vcs: String = sqlx::query_scalar("SELECT vcs FROM projects WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?
            .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
        if vcs != "git" {
            return Err((
                StatusCode::BAD_REQUEST,
                "this project has no version control, so runs edit your files directly —                  there is no isolated worktree to review or roll back. Working without                  prompts needs a git project."
                    .into(),
            ));
        }
    }

    // `PROJECT_COLUMNS` and `project_json`, not a hand-written pair.
    //
    // This used to return its own shorter list, which omitted `kind` and
    // `github_repo` — and the client types the response as a whole `Project`
    // and puts it straight into state. So one click on the autonomy toggle made
    // the GitHub chip and the Import-issues button disappear until a reload,
    // because the fields behind them came back absent rather than unchanged.
    // `$n IS NOT DISTINCT FROM` would be neater but cannot express "leave it";
    // the paired boolean is what carries absent-versus-null through to SQL.
    let row = sqlx::query(&format!(
        "UPDATE projects
            SET full_auto_opt_in = COALESCE($2, full_auto_opt_in),
                default_branch   = COALESCE($3, default_branch),
                name             = COALESCE($4, name),
                default_engine   = CASE WHEN $5 THEN $6  ELSE default_engine END,
                default_tier     = CASE WHEN $7 THEN $8  ELSE default_tier   END,
                default_effort   = CASE WHEN $9 THEN $10 ELSE default_effort END
          WHERE id = $1
      RETURNING {PROJECT_COLUMNS}"
    ))
    .bind(id)
    .bind(body.full_auto_opt_in)
    .bind(body.default_branch.as_deref())
    .bind(name.as_deref())
    .bind(body.default_engine.is_some())
    .bind(body.default_engine.clone().flatten())
    .bind(body.default_tier.is_some())
    .bind(body.default_tier.flatten().map(|t| t.as_str().to_string()))
    .bind(body.default_effort.is_some())
    .bind(body.default_effort.flatten().map(|e| e.as_str().to_string()))
    .fetch_optional(&state.db.pool)
    .await
    .map_err(internal)?
    .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;

    Ok(Json(project_json(&row)))
}
