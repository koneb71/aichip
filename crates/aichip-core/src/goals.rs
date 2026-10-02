//! Goals: what the work is for.
//!
//! A workspace's goals form a tree — a company goal, the goals under it — and
//! cards and routines point at the goal they serve. Three things follow:
//!
//! - **Every run of a card is told why.** The chain from the top goal down to
//!   the card's own, and the epic it belongs to, go into its prompt as a
//!   fenced "Why this matters" — so an agent settles the small choices the way
//!   the person would. Fenced because titles and descriptions are anyone's
//!   words, and capped because it is background, not the brief.
//! - **A manager pass sees the goals**, with how far along each is.
//! - **Progress is counted, never stored**: done cards over all cards in the
//!   goal's subtree, whenever it is asked — it cannot drift from the board.
//!
//! The tree is kept a tree the way `org_chart` keeps its own: one writer of
//! `parent_id`, a lock per workspace, no cycle, bounded depth.

use crate::db::Db;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

pub const MAX_DEPTH: i32 = 6;
/// How much of the chain a run is handed, at most.
pub const MAX_CONTEXT_CHARS: usize = 1800;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Refused {
    #[error("a goal cannot sit under itself")]
    Itself,
    #[error("that goal is in another workspace")]
    OtherWorkspace,
    #[error("that goal is under this one already — that would be a loop")]
    Cycle,
    #[error("that would make goals deeper than {MAX_DEPTH} levels")]
    TooDeep,
    #[error("no such goal")]
    Missing,
    #[error("{0}")]
    Invalid(String),
}

/// One goal, with its progress counted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub status: String,
    pub target_date: Option<NaiveDate>,
    pub position: f64,
    /// Cards in this goal's subtree that are done, and all of them.
    pub done: i64,
    pub total: i64,
    pub created_at: DateTime<Utc>,
}

pub fn vet_title(title: &str) -> Result<String, Refused> {
    let t = title.trim();
    if t.is_empty() || t.chars().count() > 200 {
        return Err(Refused::Invalid(
            "a goal needs a title under 200 characters".into(),
        ));
    }
    Ok(t.to_string())
}

pub fn vet_status(status: &str) -> Result<(), Refused> {
    match status {
        "active" | "achieved" | "abandoned" => Ok(()),
        _ => Err(Refused::Invalid(
            "status is active, achieved or abandoned".into(),
        )),
    }
}

/// Every goal of a workspace, with progress over each one's subtree.
pub async fn list(db: &Db, workspace: Uuid) -> anyhow::Result<Vec<Goal>> {
    let rows = sqlx::query(
        "WITH RECURSIVE under(root, id, depth) AS (
             SELECT id, id, 0 FROM goals WHERE workspace_id = $1
             UNION ALL
             SELECT u.root, g.id, u.depth + 1 FROM goals g JOIN under u ON g.parent_id = u.id
              WHERE u.depth < 12)
         SELECT g.id, g.parent_id, g.title, g.description, g.status, g.target_date, g.position,
                g.created_at,
                COUNT(t.id) FILTER (WHERE t.board_column = 'done') AS done,
                COUNT(t.id) AS total
           FROM goals g
           LEFT JOIN under u ON u.root = g.id
           LEFT JOIN tasks t ON t.goal_id = u.id
          WHERE g.workspace_id = $1
          GROUP BY g.id
          ORDER BY g.position, g.created_at",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows.iter().map(goal_from).collect())
}

fn goal_from(r: &sqlx::postgres::PgRow) -> Goal {
    Goal {
        id: r.get("id"),
        parent_id: r.get("parent_id"),
        title: r.get("title"),
        description: r.get("description"),
        status: r.get("status"),
        target_date: r.get("target_date"),
        position: r.get("position"),
        done: r.get("done"),
        total: r.get("total"),
        created_at: r.get("created_at"),
    }
}

pub async fn get(db: &Db, id: Uuid) -> anyhow::Result<Option<(Uuid, Goal)>> {
    let ws: Option<Uuid> = sqlx::query_scalar("SELECT workspace_id FROM goals WHERE id = $1")
        .bind(id)
        .fetch_optional(&db.pool)
        .await?;
    let Some(ws) = ws else { return Ok(None) };
    Ok(list(db, ws)
        .await?
        .into_iter()
        .find(|g| g.id == id)
        .map(|g| (ws, g)))
}

/// A goal and everything under it.
pub async fn subtree(db: &Db, id: Uuid) -> anyhow::Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar(
        "WITH RECURSIVE down(id, depth) AS (
             SELECT id, 0 FROM goals WHERE id = $1
             UNION ALL
             SELECT g.id, d.depth + 1 FROM goals g JOIN down d ON g.parent_id = d.id
              WHERE d.depth < 12)
         SELECT id FROM down ORDER BY depth",
    )
    .bind(id)
    .fetch_all(&db.pool)
    .await?)
}

/// From the top goal down to this one: (title, description).
pub async fn chain(db: &Db, id: Uuid) -> anyhow::Result<Vec<(String, String)>> {
    Ok(sqlx::query_as(
        "WITH RECURSIVE up(id, parent_id, title, description, depth) AS (
             SELECT id, parent_id, title, description, 0 FROM goals WHERE id = $1
             UNION ALL
             SELECT g.id, g.parent_id, g.title, g.description, u.depth + 1
               FROM goals g JOIN up u ON g.id = u.parent_id
              WHERE u.depth < 12)
         SELECT title, description FROM up ORDER BY depth DESC",
    )
    .bind(id)
    .fetch_all(&db.pool)
    .await?)
}

async fn depth_of(db: &Db, id: Uuid) -> anyhow::Result<i32> {
    Ok(chain(db, id).await?.len() as i32 - 1)
}

async fn height_of(db: &Db, id: Uuid) -> anyhow::Result<i32> {
    Ok(sqlx::query_scalar::<_, Option<i32>>(
        "WITH RECURSIVE down(id, depth) AS (
             SELECT id, 0 FROM goals WHERE id = $1
             UNION ALL
             SELECT g.id, d.depth + 1 FROM goals g JOIN down d ON g.parent_id = d.id
              WHERE d.depth < 12)
         SELECT max(depth) FROM down",
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await?
    .unwrap_or(0))
}

/// Can `goal` (None for a new one, in `workspace`) sit under `parent`?
async fn vet_parent(
    db: &Db,
    workspace: Uuid,
    goal: Option<Uuid>,
    parent: Uuid,
) -> anyhow::Result<Result<(), Refused>> {
    if goal == Some(parent) {
        return Ok(Err(Refused::Itself));
    }
    let parent_ws: Option<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM goals WHERE id = $1")
            .bind(parent)
            .fetch_optional(&db.pool)
            .await?;
    match parent_ws {
        None => return Ok(Err(Refused::Missing)),
        Some(ws) if ws != workspace => return Ok(Err(Refused::OtherWorkspace)),
        _ => {}
    }
    let height = match goal {
        Some(g) => {
            if subtree(db, g).await?.contains(&parent) {
                return Ok(Err(Refused::Cycle));
            }
            height_of(db, g).await?
        }
        None => 0,
    };
    if depth_of(db, parent).await? + 1 + height > MAX_DEPTH - 1 {
        return Ok(Err(Refused::TooDeep));
    }
    Ok(Ok(()))
}

async fn lock(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace: Uuid,
) -> anyhow::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('aichip.goals:' || $1::text))")
        .bind(workspace)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub struct NewGoal {
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub target_date: Option<NaiveDate>,
}

pub async fn create(db: &Db, workspace: Uuid, g: NewGoal) -> anyhow::Result<Result<Uuid, Refused>> {
    let title = match vet_title(&g.title) {
        Ok(t) => t,
        Err(e) => return Ok(Err(e)),
    };
    let mut tx = db.pool.begin().await?;
    lock(&mut tx, workspace).await?;
    if let Some(parent) = g.parent_id {
        if let Err(e) = vet_parent(db, workspace, None, parent).await? {
            return Ok(Err(e));
        }
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO goals (workspace_id, parent_id, title, description, target_date, position)
         VALUES ($1, $2, $3, $4, $5,
                 COALESCE((SELECT max(position) + 1 FROM goals
                            WHERE workspace_id = $1 AND parent_id IS NOT DISTINCT FROM $2), 0))
         RETURNING id",
    )
    .bind(workspace)
    .bind(g.parent_id)
    .bind(title)
    .bind(g.description.trim().chars().take(4000).collect::<String>())
    .bind(g.target_date)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Ok(id))
}

#[derive(Default)]
pub struct GoalPatch {
    pub parent_id: Option<Option<Uuid>>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub target_date: Option<Option<NaiveDate>>,
}

pub async fn update(db: &Db, id: Uuid, p: GoalPatch) -> anyhow::Result<Result<(), Refused>> {
    let Some(workspace): Option<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM goals WHERE id = $1")
            .bind(id)
            .fetch_optional(&db.pool)
            .await?
    else {
        return Ok(Err(Refused::Missing));
    };
    let title = match p.title.as_deref().map(vet_title).transpose() {
        Ok(t) => t,
        Err(e) => return Ok(Err(e)),
    };
    if let Some(s) = &p.status {
        if let Err(e) = vet_status(s) {
            return Ok(Err(e));
        }
    }
    let mut tx = db.pool.begin().await?;
    lock(&mut tx, workspace).await?;
    if let Some(Some(parent)) = p.parent_id {
        if let Err(e) = vet_parent(db, workspace, Some(id), parent).await? {
            return Ok(Err(e));
        }
    }
    sqlx::query(
        "UPDATE goals SET
            parent_id   = CASE WHEN $2 THEN $3 ELSE parent_id END,
            title       = COALESCE($4, title),
            description = COALESCE($5, description),
            status      = COALESCE($6, status),
            target_date = CASE WHEN $7 THEN $8 ELSE target_date END,
            updated_at  = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(p.parent_id.is_some())
    .bind(p.parent_id.flatten())
    .bind(title)
    .bind(
        p.description
            .map(|d| d.trim().chars().take(4000).collect::<String>()),
    )
    .bind(p.status)
    .bind(p.target_date.is_some())
    .bind(p.target_date.flatten())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Ok(()))
}

/// Delete a goal. Its children move up to its parent, and the cards that
/// served it serve its parent — nothing below it is left hanging.
pub async fn delete(db: &Db, id: Uuid) -> anyhow::Result<bool> {
    let mut tx = db.pool.begin().await?;
    let workspace: Option<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM goals WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(workspace) = workspace else {
        return Ok(false);
    };
    // Serialised with create and move, which take the same lock. And the row
    // itself is locked before anything moves: a card or goal pointed at it
    // by a writer that does not take the lock holds a key-share lock on it,
    // so this waits for that writer and then moves what it wrote — or makes
    // it wait and fail on the foreign key — instead of letting the delete's
    // `ON DELETE SET NULL` quietly orphan it.
    lock(&mut tx, workspace).await?;
    let parent: Option<Option<Uuid>> =
        sqlx::query_scalar("SELECT parent_id FROM goals WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(parent) = parent else {
        return Ok(false);
    };
    for sql in [
        "UPDATE goals SET parent_id = $2 WHERE parent_id = $1",
        "UPDATE tasks SET goal_id = $2 WHERE goal_id = $1",
        "UPDATE routines SET goal_id = $2 WHERE goal_id = $1",
    ] {
        sqlx::query(sql)
            .bind(id)
            .bind(parent)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM goals WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// A goal by title, for an agent that names one. Case-insensitive, within the
/// workspace; an unknown title is refused with the real ones.
///
/// Titles are not unique — "Polish" under two different goals is ordinary —
/// so a title that matches more than one is refused too, listing each by its
/// path, and the path is accepted: `Growth > Polish`. Picking one would file
/// the card under the wrong goal and roll its progress up the wrong tree,
/// without anyone being told.
pub async fn by_title(
    db: &Db,
    workspace: Uuid,
    title: &str,
) -> anyhow::Result<Result<Uuid, String>> {
    let wanted: Vec<String> = title
        .split('>')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect();
    let Some(last) = wanted.last() else {
        return Ok(Err("name a goal by its title".to_string()));
    };
    let hits: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM goals WHERE workspace_id = $1 AND lower(title) = $2 AND status = 'active'
          ORDER BY created_at",
    )
    .bind(workspace)
    .bind(last)
    .fetch_all(&db.pool)
    .await?;
    let mut matching: Vec<(Uuid, String)> = vec![];
    for id in hits {
        let path: Vec<String> = chain(db, id).await?.into_iter().map(|(t, _)| t).collect();
        let lower: Vec<String> = path.iter().map(|t| t.to_lowercase()).collect();
        if lower.ends_with(&wanted) {
            matching.push((id, path.join(" > ")));
        }
    }
    match matching.as_slice() {
        [(id, _)] => return Ok(Ok(*id)),
        [] => {}
        several => {
            let paths: Vec<&str> = several.iter().map(|(_, p)| p.as_str()).collect();
            return Ok(Err(format!(
                "more than one active goal is called \"{}\": {}. Name it by its path, as written here.",
                title.trim(),
                paths.join("; ")
            )));
        }
    }
    let known: Vec<String> = sqlx::query_scalar(
        "SELECT title FROM goals WHERE workspace_id = $1 AND status = 'active' ORDER BY title",
    )
    .bind(workspace)
    .fetch_all(&db.pool)
    .await?;
    Ok(Err(if known.is_empty() {
        "this workspace has no active goals".to_string()
    } else {
        format!(
            "no active goal called \"{}\". The goals are: {}",
            title.trim(),
            known.join(", ")
        )
    }))
}

/// The goal a manager pass's routine serves, which a card it creates takes
/// on unless it names its own.
pub async fn of_pass(db: &Db, pass_id: Uuid) -> anyhow::Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT rt.goal_id FROM routine_runs rr JOIN routines rt ON rt.id = rr.routine_id
          WHERE rr.id = $1",
    )
    .bind(pass_id)
    .fetch_optional(&db.pool)
    .await?
    .flatten())
}

/// "Why this matters" for a card's run: the goal chain from the top, and the
/// epic it belongs to. `None` when the card serves no goal and has no epic.
pub async fn why_this_matters(db: &Db, task_id: Uuid) -> anyhow::Result<Option<String>> {
    let row = sqlx::query(
        "SELECT t.goal_id, p.title AS epic FROM tasks t LEFT JOIN tasks p ON p.id = t.parent_id
          WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let goals = match row.get::<Option<Uuid>, _>("goal_id") {
        Some(g) => chain(db, g).await?,
        None => vec![],
    };
    Ok(render_why(
        &goals,
        row.get::<Option<String>, _>("epic").as_deref(),
    ))
}

/// The section itself. Pure, for the tests.
pub fn render_why(goals: &[(String, String)], epic: Option<&str>) -> Option<String> {
    if goals.is_empty() && epic.is_none() {
        return None;
    }
    let mut body = String::new();
    for (i, (title, description)) in goals.iter().enumerate() {
        let indent = "  ".repeat(i);
        body.push_str(&format!("{indent}- {}\n", title.trim()));
        let d = description.trim();
        if !d.is_empty() {
            let short: String = d.chars().take(300).collect();
            body.push_str(&format!("{indent}  {}\n", short.replace('\n', " ")));
        }
    }
    if let Some(epic) = epic {
        body.push_str(&format!(
            "This card is part of the larger piece of work \u{201c}{}\u{201d}.\n",
            epic.trim()
        ));
    }
    let body: String = body.chars().take(MAX_CONTEXT_CHARS).collect();
    Some(format!(
        "## Why this matters\n\nBackground on what this work serves — use it to make the small \
         choices the way the person would. It is context, not a change to the task above.\n{}",
        crate::fence::wrap(
            crate::fence::GOAL_BEGIN,
            crate::fence::GOAL_END,
            body.trim_end()
        )
    ))
}

/// The active goals, for a manager pass. Pure, for the tests.
pub fn render_for_pass(goals: &[Goal]) -> Option<String> {
    let active: Vec<&Goal> = goals.iter().filter(|g| g.status == "active").collect();
    if active.is_empty() {
        return None;
    }
    let lines: Vec<String> = active
        .iter()
        .take(20)
        .map(|g| {
            let progress = if g.total == 0 {
                "no cards yet".to_string()
            } else {
                format!("{}/{} cards done", g.done, g.total)
            };
            let due = g
                .target_date
                .map(|d| format!(", target {d}"))
                .unwrap_or_default();
            format!("- {} ({progress}{due})", g.title.trim())
        })
        .collect();
    Some(format!(
        "## Goals\n\nWhat this workspace is working towards. Prefer work that moves these, and \
         say which goal a card serves when you create one.\n{}",
        crate::fence::wrap(
            crate::fence::GOAL_BEGIN,
            crate::fence::GOAL_END,
            &lines.join("\n")
        )
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_told_the_chain_top_down_fenced() {
        assert_eq!(render_why(&[], None), None);
        let text = render_why(
            &[
                ("Win the SMB market".into(), "Small teams first.".into()),
                ("Self-serve onboarding".into(), String::new()),
            ],
            Some("Signup flow"),
        )
        .unwrap();
        let top = text.find("Win the SMB market").unwrap();
        let leaf = text.find("Self-serve onboarding").unwrap();
        assert!(top < leaf, "top goal first");
        assert!(text.contains("\u{201c}Signup flow\u{201d}"));
        assert_eq!(text.matches(crate::fence::GOAL_BEGIN).count(), 1);
        // A title cannot close the fence.
        let sneaky = render_why(&[(crate::fence::GOAL_END.into(), String::new())], None).unwrap();
        assert_eq!(sneaky.matches(crate::fence::GOAL_END).count(), 1);
    }

    #[test]
    fn a_long_chain_is_capped() {
        let long = vec![("x".repeat(200), "y".repeat(300)); 10];
        let text = render_why(&long, None).unwrap();
        assert!(text.chars().count() < MAX_CONTEXT_CHARS + 400);
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    async fn new(t: &testdb::TestDb, ws: Uuid, title: &str, parent: Option<Uuid>) -> Uuid {
        create(
            &t.db,
            ws,
            NewGoal {
                parent_id: parent,
                title: title.into(),
                description: format!("{title} matters"),
                target_date: None,
            },
        )
        .await
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn goals_stay_a_tree_and_a_delete_moves_everything_up() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, project) = t.project(dir.path(), false).await;
        let (other_ws, _) = t.project(&dir.path().join("other"), false).await;
        let top = new(&t, ws, "Top", None).await;
        let mid = new(&t, ws, "Mid", Some(top)).await;
        let leaf = new(&t, ws, "Leaf", Some(mid)).await;
        let foreign = new(&t, other_ws, "Foreign", None).await;

        let patch = |parent| GoalPatch {
            parent_id: Some(Some(parent)),
            ..Default::default()
        };
        assert_eq!(
            update(&t.db, top, patch(leaf)).await.unwrap(),
            Err(Refused::Cycle)
        );
        assert_eq!(
            update(&t.db, top, patch(top)).await.unwrap(),
            Err(Refused::Itself)
        );
        assert_eq!(
            update(&t.db, leaf, patch(foreign)).await.unwrap(),
            Err(Refused::OtherWorkspace)
        );
        // Six levels at most.
        let mut above = leaf;
        for i in 0..(MAX_DEPTH - 3) {
            above = new(&t, ws, &format!("L{i}"), Some(above)).await;
        }
        let too_deep = create(
            &t.db,
            ws,
            NewGoal {
                parent_id: Some(above),
                title: "Too deep".into(),
                description: String::new(),
                target_date: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(too_deep, Err(Refused::TooDeep));

        // Deleting the middle: its child and its cards move up to Top.
        let card = t.card(project, "c").await;
        sqlx::query("UPDATE tasks SET goal_id = $2 WHERE id = $1")
            .bind(card)
            .bind(mid)
            .execute(&t.db.pool)
            .await
            .unwrap();
        assert!(delete(&t.db, mid).await.unwrap());
        let parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM goals WHERE id = $1")
            .bind(leaf)
            .fetch_one(&t.db.pool)
            .await
            .unwrap();
        assert_eq!(parent, Some(top));
        let goal: Option<Uuid> = sqlx::query_scalar("SELECT goal_id FROM tasks WHERE id = $1")
            .bind(card)
            .fetch_one(&t.db.pool)
            .await
            .unwrap();
        assert_eq!(goal, Some(top));
        t.finish().await;
    }

    #[tokio::test]
    async fn progress_rolls_up_and_an_epics_cards_serve_its_goal() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, project) = t.project(dir.path(), false).await;
        let top = new(&t, ws, "Top", None).await;
        let leaf = new(&t, ws, "Leaf", Some(top)).await;
        let epic = t.card(project, "Signup flow").await;
        sqlx::query("UPDATE tasks SET goal_id = $2 WHERE id = $1")
            .bind(epic)
            .bind(leaf)
            .execute(&t.db.pool)
            .await
            .unwrap();
        // A card split out of the epic, by whatever path: it serves the goal.
        let child: Uuid = sqlx::query_scalar(
            "INSERT INTO tasks (project_id, title, prompt, parent_id, board_column)
             VALUES ($1, 'child', 'child', $2, 'done') RETURNING id",
        )
        .bind(project)
        .bind(epic)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        let goal: Option<Uuid> = sqlx::query_scalar("SELECT goal_id FROM tasks WHERE id = $1")
            .bind(child)
            .fetch_one(&t.db.pool)
            .await
            .unwrap();
        assert_eq!(goal, Some(leaf));

        let all = list(&t.db, ws).await.unwrap();
        let of = |id: Uuid| {
            all.iter()
                .find(|g| g.id == id)
                .map(|g| (g.done, g.total))
                .unwrap()
        };
        assert_eq!(of(leaf), (1, 2));
        assert_eq!(of(top), (1, 2), "the top goal counts everything under it");

        // The child's run is told why, top down, with its epic.
        let why = why_this_matters(&t.db, child).await.unwrap().unwrap();
        assert!(why.find("Top").unwrap() < why.find("Leaf").unwrap());
        assert!(why.contains("\u{201c}Signup flow\u{201d}"));
        assert!(why.contains(crate::fence::GOAL_BEGIN));
        let plain = t.card(project, "no goal").await;
        assert_eq!(why_this_matters(&t.db, plain).await.unwrap(), None);
        t.finish().await;
    }

    #[tokio::test]
    async fn a_manager_pass_serves_its_routines_goal() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, project) = t.project(dir.path(), false).await;
        let goal = new(&t, ws, "Retention", None).await;
        let routine: Uuid = sqlx::query_scalar(
            "INSERT INTO routines (workspace_id, name, kind, project_id, prompt, cron_expr, goal_id)
             VALUES ($1, 'm', 'manage', $2, '', '0 9 * * *', $3) RETURNING id",
        )
        .bind(ws)
        .bind(project)
        .bind(goal)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        let pass: Uuid = sqlx::query_scalar(
            "INSERT INTO routine_runs (routine_id, trigger) VALUES ($1, 'schedule') RETURNING id",
        )
        .bind(routine)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        assert_eq!(of_pass(&t.db, pass).await.unwrap(), Some(goal));
        assert_eq!(by_title(&t.db, ws, "retention").await.unwrap(), Ok(goal));
        assert!(by_title(&t.db, ws, "Growth")
            .await
            .unwrap()
            .unwrap_err()
            .contains("Retention"));
        t.finish().await;
    }

    #[tokio::test]
    async fn two_goals_with_one_title_are_told_apart_by_their_path() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, _) = t.project(dir.path(), false).await;
        let growth = new(&t, ws, "Growth", None).await;
        let infra = new(&t, ws, "Infra", None).await;
        let a = new(&t, ws, "Polish", Some(growth)).await;
        let b = new(&t, ws, "Polish", Some(infra)).await;

        let refused = by_title(&t.db, ws, "polish").await.unwrap().unwrap_err();
        assert!(
            refused.contains("Growth > Polish") && refused.contains("Infra > Polish"),
            "{refused}"
        );
        assert_eq!(by_title(&t.db, ws, "Growth > Polish").await.unwrap(), Ok(a));
        assert_eq!(by_title(&t.db, ws, "infra>polish").await.unwrap(), Ok(b));
        assert!(by_title(&t.db, ws, "Ops > Polish").await.unwrap().is_err());
        // One of a kind is still found by its title alone.
        assert_eq!(by_title(&t.db, ws, "growth").await.unwrap(), Ok(growth));
        t.finish().await;
    }

    /// A card pointed at a goal by a writer that does not take the goals
    /// lock — the card's own PATCH — while the goal is being deleted moves
    /// up with the rest, rather than being left serving nothing.
    #[tokio::test]
    async fn a_card_pointed_at_a_goal_as_it_is_deleted_moves_up_with_the_rest() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, project) = t.project(dir.path(), false).await;
        let top = new(&t, ws, "Top", None).await;
        let doomed = new(&t, ws, "Doomed", Some(top)).await;
        let card = t.card(project, "late").await;

        // The card's write is in flight, uncommitted, when the delete starts.
        let mut writer = t.db.pool.begin().await.unwrap();
        sqlx::query("UPDATE tasks SET goal_id = $2 WHERE id = $1")
            .bind(card)
            .bind(doomed)
            .execute(&mut *writer)
            .await
            .unwrap();
        let db = t.db.clone();
        let deleting = tokio::spawn(async move { delete(&db, doomed).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(!deleting.is_finished(), "the delete waits for the writer");
        writer.commit().await.unwrap();
        assert!(deleting.await.unwrap());

        let goal: Option<Uuid> = sqlx::query_scalar("SELECT goal_id FROM tasks WHERE id = $1")
            .bind(card)
            .fetch_one(&t.db.pool)
            .await
            .unwrap();
        assert_eq!(goal, Some(top));
        t.finish().await;
    }
}
