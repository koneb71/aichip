//! Board cards, written from the core.
//!
//! Everything else that creates a task does it from an HTTP route, where the
//! insert is tangled up with route concerns — nullable permission modes, engine
//! defaulting, claiming attachments and knowledge-base articles, vetting, then
//! enqueuing. None of that applies to a card the *system* creates on a person's
//! behalf, which is what an epic's sub-tickets are.
//!
//! This module is deliberately narrow: it makes a child card and it looks up an
//! agent by name. It is not a general task API, and a future caller wanting one
//! should build that rather than widen this.

use crate::db::Db;
use sqlx::Row;
use uuid::Uuid;

/// A sub-ticket to be created under an epic.
pub struct NewChildTask {
    pub parent_id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub prompt: String,
    pub agent_id: Option<Uuid>,
    pub engine: String,
    /// Copied from the epic, so re-running this ticket continues on the epic's
    /// branch instead of forking a second one off the default branch and
    /// redoing work that is already committed there.
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    /// Plan order — 1, 2, 3 — not a board position. See below.
    pub order_hint: f64,
}

/// Create a sub-ticket. Returns its id.
pub async fn create_child(db: &Db, spec: NewChildTask) -> anyhow::Result<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        // `position` is the epoch-plus-order expression rather than the plan
        // index, and that is not a detail: `tasks.position` counts seconds since
        // 1970 (migration 0011), while a plan's positions are 1, 2, 3. Copying
        // one into the other would sort every sub-ticket above every real card
        // in its column, permanently.
        //
        // `team_id` stays NULL: a sub-ticket is one person's work, and handing it
        // to another team is how an epic ends up inside an epic.
        "INSERT INTO tasks
            (project_id, parent_id, title, prompt, agent_id, engine,
             board_column, position, worktree_path, branch)
         VALUES ($1,$2,$3,$4,$5,$6,'backlog',
                 extract(epoch FROM now()) + $7, $8, $9)
         RETURNING id",
    )
    .bind(spec.project_id)
    .bind(spec.parent_id)
    .bind(&spec.title)
    .bind(&spec.prompt)
    .bind(spec.agent_id)
    .bind(&spec.engine)
    .bind(spec.order_hint)
    .bind(&spec.worktree_path)
    .bind(&spec.branch)
    .fetch_one(&db.pool)
    .await?;
    Ok(id)
}

/// A card imported from somewhere outside aichip.
pub struct NewImportedTask {
    pub project_id: Uuid,
    pub title: String,
    /// Already framed — see `github::issues::issue_prompt`. Never a raw body.
    pub prompt: String,
    pub engine: String,
    /// `github_issue`. A discriminator, because what follows from it is
    /// specific and the next importer must not inherit it.
    pub source: String,
    /// `owner/repo#42`, which is what makes a second import a no-op.
    pub source_ref: String,
    pub source_url: String,
    pub source_number: i32,
    /// Position within this import, so a batch keeps the order it was listed in.
    pub order_hint: f64,
}

/// Create a card from an imported issue, or say it was already there.
///
/// `Ok(None)` means a card for this issue already exists — the ordinary answer
/// to importing twice, not a failure. That is the partial-unique index doing
/// the work, the same idiom `previews::start` uses for a double-click: the
/// second insert loses rather than producing a duplicate.
///
/// Two things this deliberately does not do, both for the same reason — the
/// text came from outside:
///
/// * **It never starts a run.** The card lands in `backlog` with no run and no
///   enqueue, and this function has no parameter that could change that. On a
///   public repository the person who wrote the prompt is a stranger, and the
///   one place a human has to stand is between them and an agent holding Write
///   and Bash.
/// * **`parent_id` stays NULL.** An imported issue is a top-level request, not
///   a sub-ticket some plan produced, and the epic roll-up counts children.
pub async fn create_imported(db: &Db, spec: NewImportedTask) -> anyhow::Result<Option<Uuid>> {
    let id: Option<Uuid> = sqlx::query_scalar(
        // `plan_first` because a plan is a second, cheaper look at what the
        // issue actually asked for, approved before a file is touched. A
        // default rather than a lock — it is editable on the card.
        "INSERT INTO tasks
            (project_id, title, prompt, engine, board_column, position, plan_first,
             source, source_ref, source_url, source_number)
         VALUES ($1,$2,$3,$4,'backlog', extract(epoch FROM now()) + $5, TRUE,
                 $6,$7,$8,$9)
         ON CONFLICT DO NOTHING
         RETURNING id",
    )
    .bind(spec.project_id)
    .bind(&spec.title)
    .bind(&spec.prompt)
    .bind(&spec.engine)
    .bind(spec.order_hint)
    .bind(&spec.source)
    .bind(&spec.source_ref)
    .bind(&spec.source_url)
    .bind(spec.source_number)
    .fetch_optional(&db.pool)
    .await?;
    Ok(id)
}

/// Find the agent a plan named, within the project's workspace.
///
/// Scoped through the project on purpose. `agents.name` stopped being globally
/// unique in migration 0002 — it is `UNIQUE (workspace_id, name)` — so a bare
/// lookup by name can return another workspace's agent, and every later edit of
/// the card would then be refused as cross-workspace.
pub async fn resolve_agent_by_name(
    db: &Db,
    project_id: Uuid,
    name: &str,
) -> anyhow::Result<Option<Uuid>> {
    let row = sqlx::query(
        "SELECT a.id FROM agents a
           JOIN projects p ON p.workspace_id = a.workspace_id
          WHERE p.id = $1 AND lower(a.name) = lower($2)
          LIMIT 1",
    )
    .bind(project_id)
    .bind(name)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(|r| r.get("id")))
}
