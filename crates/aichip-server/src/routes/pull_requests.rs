//! Finishing a card as a pull request instead of a local merge.
//!
//! Its own module rather than more of `tasks.rs`, following `previews.rs`: a
//! sub-resource of a task with its own lifecycle and its own refusals.
//!
//! ## Refusals are data on the way in and errors on the way out
//!
//! `GET` returns *why not* alongside the record, so a card with no remote
//! renders the reason without a failed request — the shape `DockerStatus`
//! already uses to gate the preview panel. `POST` turns the same facts into a
//! 409 at the click, which is how this codebase refuses everywhere else: at
//! the moment somebody asked, never by silently doing something lesser.

use crate::routes::{internal, ApiError};
use crate::AppState;
use aichip_core::github::{self, pr, GhError};
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tasks/{id}/pull-request", get(show).post(open))
        .route("/tasks/{id}/pull-request/refresh", post(refresh))
        .route("/projects/{id}/github/issues", get(list_issues))
        .route("/projects/{id}/github/issues/import", post(import_issues))
}

/// What a card needs before it can have a pull request at all.
struct Card {
    title: String,
    prompt: String,
    project_id: Uuid,
    project_path: String,
    default_branch: String,
    worktree: aichip_core::worktrees::manager::Worktree,
    pr_number: Option<i32>,
    /// Set when this card was imported from a GitHub issue, so the pull
    /// request can say `Closes #42` and let GitHub close it on merge.
    source: Option<String>,
    source_ref: Option<String>,
    source_number: Option<i32>,
}

/// Load a card, or say precisely which precondition it fails.
///
/// Ordered most-specific-first so the message is the most useful true one: a
/// project with no version control is a different conversation from a card
/// whose run has not produced a worktree yet.
async fn card(state: &AppState, id: Uuid) -> Result<Card, ApiError> {
    let row = sqlx::query(
        "SELECT t.title, t.prompt, t.worktree_path, t.branch, t.pr_number,
                t.source, t.source_ref, t.source_number,
                t.project_id, p.path AS project_path, p.default_branch, p.vcs
           FROM tasks t JOIN projects p ON p.id = t.project_id
          WHERE t.id = $1",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    if row.get::<String, _>("vcs") != "git" {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "this project has no version control, so its tasks edit the folder \
             directly — there is no branch to open a pull request from"
                .into(),
        ));
    }
    let (Some(path), Some(branch)): (Option<String>, Option<String>) =
        (row.get("worktree_path"), row.get("branch"))
    else {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "this card has no worktree yet, so there is no branch to push".into(),
        ));
    };
    Ok(Card {
        title: row.get("title"),
        prompt: row.get("prompt"),
        project_id: row.get("project_id"),
        project_path: row.get("project_path"),
        default_branch: row.get("default_branch"),
        worktree: aichip_core::worktrees::manager::Worktree {
            path: path.into(),
            branch,
        },
        pr_number: row.get("pr_number"),
        source: row.get("source"),
        source_ref: row.get("source_ref"),
        source_number: row.get("source_number"),
    })
}

/// Why this card cannot have a pull request opened right now, if it cannot.
///
/// Deliberately not an error on the read path: the panel wants to say "install
/// the GitHub CLI" in a dim line, not fail to load.
async fn refusal(state: &AppState, card: &Card) -> Option<String> {
    let Some(info) = github::detect().await else {
        return Some(
            "the GitHub CLI (gh) is not installed, so aichip cannot open a pull \
             request. Install it and sign in from Connections."
                .into(),
        );
    };
    if !info.usable() {
        // `gh`'s own words about the account are the actionable half.
        let problem = info
            .active()
            .and_then(|a| a.problem.clone())
            .unwrap_or_else(|| "no account is signed in".into());
        return Some(format!(
            "the GitHub CLI is installed but not usable: {problem}"
        ));
    }
    // Resolving doubles as the check: no `origin` that parses as a GitHub
    // repository means there is nowhere to open one. It also *remembers* the
    // answer, so the next render and every poll tick read a column instead of
    // spawning git.
    if aichip_core::github::repo::resolve(&state.db, card.project_id)
        .await
        .is_none()
    {
        return Some(
            "this project has no GitHub `origin` remote, so there is nowhere to \
             open a pull request."
                .into(),
        );
    }
    None
}

/// What the drawer renders.
async fn show(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = sqlx::query(
        "SELECT pr_url, pr_number, pr_state, pr_checks, pr_review, pr_synced_at
           FROM tasks WHERE id = $1",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    let stored = stored_json(&row);
    // A card that fails a precondition still renders — with the reason, not an
    // error. `card()` errors only for the two structural cases, which are also
    // worth showing rather than throwing.
    let (refusal_text, can_open) = match card(&state, id).await {
        Ok(c) => {
            let why = refusal(&state, &c).await;
            (why.clone(), why.is_none())
        }
        Err((_, why)) => (Some(why), false),
    };
    Ok(Json(json!({
        "pr": stored,
        "canOpen": can_open,
        "refusal": refusal_text,
    })))
}

fn stored_json(row: &sqlx::postgres::PgRow) -> Value {
    let number: Option<i32> = row.get("pr_number");
    let Some(number) = number else {
        return Value::Null;
    };
    json!({
        "number": number,
        "url": row.get::<Option<String>, _>("pr_url"),
        "state": row.get::<Option<String>, _>("pr_state"),
        "checks": row.get::<Option<String>, _>("pr_checks"),
        "review": row.get::<Option<String>, _>("pr_review"),
        "syncedAt": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("pr_synced_at"),
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OpenBody {
    /// Only ever sent from the button the refusal itself offers.
    force: bool,
}

/// Push the branch, then open a pull request — or update the one there is.
///
/// Idempotent on purpose. Pressing this on a card that already has a pull
/// request pushes the follow-up commits and re-reads it, which is exactly what
/// "update my pull request" means; GitHub updates the request itself from the
/// branch. One button, two readings, both correct.
async fn open(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    body: Option<Json<OpenBody>>,
) -> Result<Json<Value>, ApiError> {
    let force = body.map(|b| b.0.force).unwrap_or(false);
    let card = card(&state, id).await?;
    if let Some(why) = refusal(&state, &card).await {
        return Err((axum::http::StatusCode::CONFLICT, why));
    }

    state
        .orchestrator
        .worktrees
        .push(&card.worktree, &format!("aichip: {}", card.title), force)
        .await
        .map_err(|e| (axum::http::StatusCode::CONFLICT, e.to_string()))?;

    // Ask before creating. This both avoids `gh`'s "already exists" error and
    // recovers a card whose stored row was lost — and it is the same call the
    // refresh makes, so there is one parser rather than a URL scraper beside it.
    let cwd = card.worktree.path.as_path();
    let existing = pr::view(cwd, &card.worktree.branch).await;
    let found = match existing {
        Ok(found) => Some(found),
        Err(GhError::NoPullRequest) => None,
        Err(e) => return Err((axum::http::StatusCode::CONFLICT, e.to_string())),
    };

    let pull = match found {
        Some(pull) => pull,
        None => {
            // Only for an issue on this very repository — GitHub's cross-repo
            // form needs write access there, so promising it would be a lie.
            let project_repo = aichip_core::github::repo::resolve(&state.db, card.project_id).await;
            let closes = pr::closes_number(
                card.source.as_deref(),
                card.source_ref.as_deref(),
                card.source_number,
                project_repo.as_deref(),
            );
            pr::create(
                cwd,
                &card.default_branch,
                &card.worktree.branch,
                &card.title,
                &pr::pr_body(&card.prompt, &card.worktree.branch, closes),
            )
            .await
            .map_err(|e| (axum::http::StatusCode::CONFLICT, e.to_string()))?;
            pr::view(cwd, &card.worktree.branch)
                .await
                .map_err(|e| (axum::http::StatusCode::CONFLICT, e.to_string()))?
        }
    };

    store(&state, id, &pull).await?;
    Ok(Json(json!({ "pr": as_json(&pull) })))
}

/// Re-read a pull request aichip already knows about.
async fn refresh(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let card = card(&state, id).await?;
    let Some(number) = card.pr_number else {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "this card has no pull request to refresh".into(),
        ));
    };
    // By number and from the *project*, not the worktree: a merged card's
    // worktree is often gone, and `gh pr view <branch>` stops resolving once
    // somebody deletes the branch.
    let pull = pr::view(
        std::path::Path::new(&card.project_path),
        &number.to_string(),
    )
    .await
    .map_err(|e| (axum::http::StatusCode::CONFLICT, e.to_string()))?;
    store(&state, id, &pull).await?;
    Ok(Json(json!({ "pr": as_json(&pull) })))
}

fn as_json(pull: &pr::PullRequest) -> Value {
    json!({
        "number": pull.number,
        "url": pull.url,
        "state": pull.state.as_str(),
        "checks": pull.checks.as_str(),
        "review": pull.review.map(|r| r.as_str()),
        "syncedAt": chrono::Utc::now(),
    })
}

/// Write what `gh` said, and move the card if the work has landed.
///
/// The move rides on a call that is already happening rather than a poller of
/// its own. `WHERE board_column = 'review'` so this can only ever finish a
/// card that was waiting to be finished — never drag one out of Backlog
/// because somebody merged its branch by hand.
///
/// It is *not* moved merely for having a pull request open: `done` means
/// landed, and the epic roll-up counts `review` and `done` alike as resolved,
/// so moving early would report progress that has not happened.
async fn store(state: &AppState, id: Uuid, pull: &pr::PullRequest) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE tasks
            SET pr_number = $2, pr_url = $3, pr_state = $4, pr_checks = $5,
                pr_review = $6, pr_synced_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(pull.number)
    .bind(&pull.url)
    .bind(pull.state.as_str())
    .bind(pull.checks.as_str())
    .bind(pull.review.map(|r| r.as_str()))
    .execute(&state.db.pool)
    .await
    .map_err(internal)?;

    if pull.state == pr::State::Merged {
        sqlx::query("UPDATE tasks SET board_column='done' WHERE id=$1 AND board_column='review'")
            .bind(id)
            .execute(&state.db.pool)
            .await
            .map_err(internal)?;
    }
    Ok(())
}

// ── Issues ──────────────────────────────────────────────────────────────────

/// The open issues on a project's repository, and which are already cards.
///
/// Already-imported issues are returned marked rather than filtered out: "why
/// isn't #42 in this list" is a worse question than seeing it greyed with a
/// link to the card it became.
pub async fn list_issues(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    use aichip_core::github;

    let Some(repo) = github::repo::resolve(&state.db, project_id).await else {
        return Ok(Json(json!({
            "repo": null,
            "issues": [],
            "refusal": "this project has no GitHub `origin` remote, so there are no \
                        issues to import.",
        })));
    };

    // What the modal needs to warn with: on a public repository the author of
    // any issue is a stranger.
    // Unknown counts as public, which is the fail-closed choice: the public
    // path is the one that warns that anyone can file an issue here.
    let public = match github::repo::parse_repo_ref(&repo) {
        Ok(parsed) => github::repo::view(&parsed)
            .await
            .map(|f| f.public)
            .unwrap_or(true),
        Err(_) => true,
    };

    let issues = match github::issues::list(&repo, 100).await {
        Ok(issues) => issues,
        Err(e) => {
            return Ok(Json(json!({
                "repo": repo,
                "issues": [],
                "refusal": e.to_string(),
            })))
        }
    };

    // One query rather than one per issue.
    let existing: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT source_ref, id FROM tasks
          WHERE project_id = $1 AND source = 'github_issue' AND source_ref IS NOT NULL",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    let already: std::collections::HashMap<String, Uuid> = existing.into_iter().collect();

    let rows: Vec<Value> = issues
        .iter()
        .map(|i| {
            let key = github::issues::source_ref(&repo, i.number);
            json!({
                "number": i.number,
                "title": i.title,
                "body": i.body,
                "url": i.url,
                "labels": i.labels,
                "author": i.author,
                "importedAs": already.get(&key),
            })
        })
        .collect();

    Ok(Json(json!({
        "repo": repo,
        "public": public,
        "issues": rows,
        "refusal": Value::Null,
    })))
}

#[derive(Deserialize)]
struct ImportIssues {
    /// Exactly the issues somebody ticked. There is no "import everything".
    numbers: Vec<i32>,
}

/// Turn chosen issues into backlog cards.
///
/// Note what this route does **not** accept: anything resembling `start`.
/// `POST /tasks` has one, and mirroring it here is the obvious convenience —
/// and it would turn "a stranger opened an issue" into "an agent is editing
/// your repository" with nobody in between. The one place a person has to
/// stand is exactly there.
pub async fn import_issues(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(body): Json<ImportIssues>,
) -> Result<Json<Value>, ApiError> {
    use aichip_core::github;
    use aichip_core::tasks::{create_imported, NewImportedTask};

    let repo = github::repo::resolve(&state.db, project_id).await.ok_or((
        axum::http::StatusCode::CONFLICT,
        "this project has no GitHub `origin` remote".to_string(),
    ))?;
    let parsed =
        github::repo::parse_repo_ref(&repo).map_err(|e| (axum::http::StatusCode::CONFLICT, e))?;
    let public = github::repo::view(&parsed)
        .await
        .map(|f| f.public)
        .unwrap_or(true);

    let wanted: std::collections::HashSet<i32> = body.numbers.iter().copied().collect();
    if wanted.is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            "choose at least one issue to import".into(),
        ));
    }

    let issues = github::issues::list(&repo, 100)
        .await
        .map_err(|e| (axum::http::StatusCode::CONFLICT, e.to_string()))?;
    let engine = state.orchestrator.default_engine();

    let mut imported = Vec::new();
    let mut skipped = Vec::new();
    for (i, issue) in issues
        .iter()
        .filter(|i| wanted.contains(&i.number))
        .enumerate()
    {
        let prompt = github::issues::issue_prompt(
            issue,
            &repo,
            github::issues::Provenance {
                author: &issue.author,
                public,
            },
        );
        let spec = NewImportedTask {
            project_id,
            title: issue.title.clone(),
            prompt,
            engine: engine.clone(),
            source: "github_issue".into(),
            source_ref: github::issues::source_ref(&repo, issue.number),
            source_url: issue.url.clone(),
            source_number: issue.number,
            order_hint: i as f64,
        };
        match create_imported(&state.db, spec).await {
            Ok(Some(id)) => imported.push(json!({ "number": issue.number, "taskId": id })),
            // Already a card. The ordinary answer to importing twice.
            Ok(None) => skipped.push(issue.number),
            Err(e) => return Err(internal(e)),
        }
    }

    Ok(Json(json!({ "imported": imported, "skipped": skipped })))
}
