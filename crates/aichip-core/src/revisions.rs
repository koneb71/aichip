//! Undo for configuration: every edit to an agent, a team, a routine, a
//! skill, a project's checks, a budget or the attention setting keeps the row
//! as it was.
//!
//! The snapshot is taken by the entity's own update handler just before it
//! writes — a source scan below holds each one to that — and holds the whole
//! row, read generically (`to_jsonb`) so a column added later is kept without
//! anyone remembering to. The table a kind reads from is a match on a closed
//! enum, never text from a request: these names are interpolated into SQL.
//!
//! Restoring is the server's job, through the same handler: what comes back
//! is validated, gated and recorded exactly like a person's edit, and the
//! restore is itself a revision of what it replaced.

use crate::db::Db;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;

/// How many revisions an entity keeps. Old enough to undo last week's edit,
/// small enough that nobody prunes.
pub const KEEP: i64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Agent,
    Team,
    Routine,
    Skill,
    ProjectChecks,
    BudgetPolicy,
    Attention,
    ReviewPolicy,
}

impl EntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntityKind::Agent => "agent",
            EntityKind::Team => "team",
            EntityKind::Routine => "routine",
            EntityKind::Skill => "skill",
            EntityKind::ProjectChecks => "project_checks",
            EntityKind::BudgetPolicy => "budget_policy",
            EntityKind::Attention => "attention",
            EntityKind::ReviewPolicy => "review_policy",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "agent" => EntityKind::Agent,
            "team" => EntityKind::Team,
            "routine" => EntityKind::Routine,
            "skill" => EntityKind::Skill,
            "project_checks" => EntityKind::ProjectChecks,
            "budget_policy" => EntityKind::BudgetPolicy,
            "attention" => EntityKind::Attention,
            "review_policy" => EntityKind::ReviewPolicy,
            _ => return None,
        })
    }

    /// The query that reads one row as JSON. Every name in it is a literal
    /// chosen here — this is the only SQL in the module built from parts.
    fn read_sql(self) -> &'static str {
        match self {
            EntityKind::Agent => "SELECT to_jsonb(x) FROM agents x WHERE x.id::text = $1",
            EntityKind::Team => "SELECT to_jsonb(x) FROM teams x WHERE x.id::text = $1",
            EntityKind::Routine => "SELECT to_jsonb(x) FROM routines x WHERE x.id::text = $1",
            EntityKind::Skill => "SELECT to_jsonb(x) FROM skills x WHERE x.id::text = $1",
            EntityKind::ProjectChecks => {
                "SELECT to_jsonb(x) FROM project_checks x WHERE x.project_id::text = $1"
            }
            EntityKind::BudgetPolicy => {
                "SELECT to_jsonb(x) FROM budget_policies x WHERE x.id::text = $1"
            }
            EntityKind::Attention => {
                "SELECT value FROM settings WHERE key = 'attention' AND $1 = 'attention'"
            }
            EntityKind::ReviewPolicy => {
                "SELECT to_jsonb(x) FROM project_review_policy x WHERE x.project_id::text = $1"
            }
        }
    }
}

/// The row as it is now, if it exists.
pub async fn current(
    db: &Db,
    kind: EntityKind,
    id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    Ok(sqlx::query_scalar::<_, serde_json::Value>(kind.read_sql())
        .bind(id)
        .fetch_optional(&db.pool)
        .await?)
}

/// Keep the row as it is, before the caller changes it. Nothing to keep
/// (a first save) is not an error. Returns the revision's id.
pub async fn snapshot(db: &Db, kind: EntityKind, id: &str) -> anyhow::Result<Option<i64>> {
    let Some(row) = current(db, kind, id).await? else {
        return Ok(None);
    };
    let rev: i64 = sqlx::query_scalar(
        "INSERT INTO config_revisions (entity_kind, entity_id, snapshot) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(kind.as_str())
    .bind(id)
    .bind(&row)
    .fetch_one(&db.pool)
    .await?;
    sqlx::query(
        "DELETE FROM config_revisions WHERE entity_kind = $1 AND entity_id = $2 AND id NOT IN (
             SELECT id FROM config_revisions WHERE entity_kind = $1 AND entity_id = $2
              ORDER BY id DESC LIMIT $3)",
    )
    .bind(kind.as_str())
    .bind(id)
    .bind(KEEP)
    .execute(&db.pool)
    .await?;
    Ok(Some(rev))
}

/// [`snapshot`], best-effort: a handler about to save must not fail the save
/// because the undo copy could not be written. Logged instead.
pub async fn keep(db: &Db, kind: EntityKind, id: &str) {
    if let Err(e) = snapshot(db, kind, id).await {
        tracing::warn!(kind = kind.as_str(), %id, error = %e, "could not keep a revision");
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub id: i64,
    pub entity_kind: String,
    pub entity_id: String,
    pub created_at: DateTime<Utc>,
    pub snapshot: serde_json::Value,
    /// Keys whose value differs from the row as it is now. What a restore
    /// would change.
    pub changed: Vec<String>,
}

/// Keys whose values differ between two rows, ignoring bookkeeping. Pure.
pub fn changed_keys(then: &serde_json::Value, now: Option<&serde_json::Value>) -> Vec<String> {
    const IGNORED: [&str; 4] = ["updated_at", "created_at", "last_fired_at", "paused_at"];
    let (Some(a), Some(b)) = (then.as_object(), now.and_then(|n| n.as_object())) else {
        return then
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
    };
    let mut keys: Vec<String> = a
        .iter()
        .filter(|(k, v)| !IGNORED.contains(&k.as_str()) && b.get(*k) != Some(*v))
        .map(|(k, _)| k.clone())
        .collect();
    keys.sort();
    keys
}

pub async fn list(db: &Db, kind: EntityKind, id: &str) -> anyhow::Result<Vec<Revision>> {
    let now = current(db, kind, id).await?;
    let rows = sqlx::query(
        "SELECT id, created_at, snapshot FROM config_revisions
          WHERE entity_kind = $1 AND entity_id = $2 ORDER BY id DESC",
    )
    .bind(kind.as_str())
    .bind(id)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let snapshot: serde_json::Value = r.get("snapshot");
            Revision {
                id: r.get("id"),
                entity_kind: kind.as_str().into(),
                entity_id: id.into(),
                created_at: r.get("created_at"),
                changed: changed_keys(&snapshot, now.as_ref()),
                snapshot,
            }
        })
        .collect())
}

pub async fn get(
    db: &Db,
    rev: i64,
) -> anyhow::Result<Option<(EntityKind, String, serde_json::Value)>> {
    let row =
        sqlx::query("SELECT entity_kind, entity_id, snapshot FROM config_revisions WHERE id = $1")
            .bind(rev)
            .fetch_optional(&db.pool)
            .await?;
    Ok(row.and_then(|r| {
        EntityKind::parse(&r.get::<String, _>("entity_kind")).map(|k| {
            (
                k,
                r.get::<String, _>("entity_id"),
                r.get::<serde_json::Value, _>("snapshot"),
            )
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_revision_names_what_a_restore_would_change() {
        let then = json!({"name": "Ada", "system_prompt": "old", "updated_at": "x"});
        let now = json!({"name": "Ada", "system_prompt": "new", "updated_at": "y"});
        assert_eq!(changed_keys(&then, Some(&now)), ["system_prompt"]);
        assert_eq!(changed_keys(&then, Some(&then)), Vec::<String>::new());
    }

    #[test]
    fn every_kind_round_trips() {
        for k in [
            EntityKind::Agent,
            EntityKind::Team,
            EntityKind::Routine,
            EntityKind::Skill,
            EntityKind::ProjectChecks,
            EntityKind::BudgetPolicy,
            EntityKind::Attention,
            EntityKind::ReviewPolicy,
        ] {
            assert_eq!(EntityKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(EntityKind::parse("users; drop table"), None);
    }

    /// Every handler that changes a revisioned entity keeps a revision first.
    /// A new writer added without one fails here, by name.
    #[test]
    fn every_writer_keeps_a_revision() {
        const WRITERS: &[(&str, &str)] = &[
            ("routes/agents.rs", "async fn update("),
            ("routes/agents.rs", "async fn remove("),
            ("routes/teams.rs", "async fn update("),
            ("routes/teams.rs", "async fn remove("),
            ("routes/routines.rs", "async fn update("),
            ("routes/routines.rs", "async fn remove("),
            ("routes/manager.rs", "async fn upsert("),
            ("routes/manager.rs", "async fn remove("),
            ("routes/skills.rs", "async fn update("),
            ("routes/skills.rs", "async fn remove("),
            ("routes/checks.rs", "async fn put_config("),
            ("routes/budgets.rs", "async fn update("),
            ("routes/budgets.rs", "async fn remove("),
            ("routes/activity.rs", "async fn set_budget("),
            ("routes/settings.rs", "async fn set_attention("),
        ];
        let server = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../aichip-server/src");
        for (file, needle) in WRITERS {
            let text = std::fs::read_to_string(server.join(file))
                .unwrap_or_else(|_| panic!("{file} is gone"));
            let start = text
                .find(needle)
                .unwrap_or_else(|| panic!("{file} has no {needle}"));
            // The body runs to the next top-level item.
            let rest = &text[start..];
            let end = rest[1..].find("\n}\n").map(|i| i + 1).unwrap_or(rest.len());
            assert!(
                rest[..end].contains("revisions::keep")
                    || rest[..end].contains("revisions::snapshot"),
                "{file} {needle} changes a revisioned entity without keeping a revision"
            );
        }
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    #[tokio::test]
    async fn an_edit_keeps_the_row_it_replaced_and_only_the_last_twenty() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let (ws, _) = t.project(dir.path(), true).await;
        let agent: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO agents (workspace_id, name, system_prompt, model_tier) VALUES ($1, 'Ada', 'v0', 'medium') RETURNING id",
        )
        .bind(ws)
        .fetch_one(&t.db.pool)
        .await
        .unwrap();
        let id = agent.to_string();
        assert_eq!(
            snapshot(&t.db, EntityKind::Agent, "not-a-row")
                .await
                .unwrap(),
            None,
            "nothing to keep is not an error"
        );
        for v in 1..=(KEEP + 3) {
            keep(&t.db, EntityKind::Agent, &id).await;
            sqlx::query("UPDATE agents SET system_prompt = $2 WHERE id = $1")
                .bind(agent)
                .bind(format!("v{v}"))
                .execute(&t.db.pool)
                .await
                .unwrap();
        }
        let revs = list(&t.db, EntityKind::Agent, &id).await.unwrap();
        assert_eq!(revs.len() as i64, KEEP, "trimmed to the newest {KEEP}");
        assert_eq!(
            revs[0].snapshot["system_prompt"],
            format!("v{}", KEEP + 2),
            "the newest is the row just replaced"
        );
        assert_eq!(revs[0].changed, ["system_prompt"]);
        let (kind, rid, snap) = get(&t.db, revs[0].id).await.unwrap().unwrap();
        assert_eq!((kind, rid.as_str()), (EntityKind::Agent, id.as_str()));
        assert_eq!(snap["name"], "Ada");
        t.finish().await;
    }
}
