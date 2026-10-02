//! The ledger: what happened, by whom.
//!
//! One writer, this module, and it never fails the thing it records — an
//! audit row that could not be written is logged and dropped, because
//! refusing to merge a card since the history table hiccupped would trade a
//! real action for a record of it.
//!
//! Three sources feed it:
//!
//! - **every mutating `/api` request**, through the server's audit layer,
//!   which sees the route's template and its path ids — never the body, which
//!   carries prompts, file contents and the occasional secret;
//! - **every agent tool call** through the three MCP endpoints, by tool name
//!   and run — never the input;
//! - **Eren itself**: a routine fired, a run reaped, a handoff, an
//!   automatic check.
//!
//! "api" is not "a person": Eren has no login, and a local process can call
//! the API as well as a browser can. The ledger says what it knows.

use crate::db::Db;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Api,
    Agent(Uuid),
    System,
}

impl Actor {
    fn kind(self) -> &'static str {
        match self {
            Actor::Api => "api",
            Actor::Agent(_) => "agent",
            Actor::System => "system",
        }
    }
    fn run(self) -> Option<Uuid> {
        match self {
            Actor::Agent(run) => Some(run),
            _ => None,
        }
    }
}

/// One thing that happened.
#[derive(Debug, Clone)]
pub struct Entry {
    pub actor: Actor,
    /// What was done, e.g. `POST /api/tasks/{id}/merge` or `tool comment`.
    pub action: String,
    /// What it was done to, when that is one thing: `tasks`, `agents`, …
    pub entity_kind: Option<String>,
    pub entity_id: Option<String>,
    /// One line, at most 200 characters.
    pub summary: String,
    /// Structured extras: changed keys, a status code, a revision id.
    pub detail: serde_json::Value,
}

impl Entry {
    pub fn new(actor: Actor, action: impl Into<String>) -> Self {
        Self {
            actor,
            action: action.into(),
            entity_kind: None,
            entity_id: None,
            summary: String::new(),
            detail: serde_json::json!({}),
        }
    }

    pub fn on(mut self, kind: &str, id: impl ToString) -> Self {
        self.entity_kind = Some(kind.to_string());
        self.entity_id = Some(id.to_string());
        self
    }

    pub fn summary(mut self, s: impl Into<String>) -> Self {
        self.summary = s.into();
        self
    }

    pub fn detail(mut self, d: serde_json::Value) -> Self {
        self.detail = d;
        self
    }
}

const MAX_SUMMARY: usize = 200;

fn clip(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > MAX_SUMMARY {
        format!("{}…", one.chars().take(MAX_SUMMARY).collect::<String>())
    } else {
        one
    }
}

/// Write one entry. Best-effort: a failure is logged, never returned.
pub async fn record(db: &Db, e: Entry) {
    let result = sqlx::query(
        "INSERT INTO audit_log (actor_kind, actor_run_id, action, entity_kind, entity_id, summary, detail)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(e.actor.kind())
    .bind(e.actor.run())
    .bind(&e.action)
    .bind(&e.entity_kind)
    .bind(&e.entity_id)
    .bind(clip(&e.summary))
    .bind(&e.detail)
    .execute(&db.pool)
    .await;
    if let Err(err) = result {
        tracing::warn!(action = %e.action, error = %err, "could not write the audit entry");
    }
}

/// How long agent and system rows are kept. What a person did through the
/// API is kept for good; the high-volume rows are not.
pub const RETENTION_DAYS: i64 = 90;

/// Drop agent and system rows past retention. The only delete this table
/// ever sees.
pub async fn prune(db: &Db) -> anyhow::Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM audit_log WHERE actor_kind <> 'api' AND at < now() - make_interval(days => $1)",
    )
    .bind(RETENTION_DAYS as i32)
    .execute(&db.pool)
    .await?
    .rows_affected())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub actor_kind: String,
    pub actor_run_id: Option<Uuid>,
    pub action: String,
    pub entity_kind: Option<String>,
    pub entity_id: Option<String>,
    pub summary: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub entity_kind: Option<String>,
    pub entity_id: Option<String>,
    pub actor_kind: Option<String>,
    /// Rows older than this id — the cursor for the next page.
    pub before: Option<i64>,
    pub limit: i64,
}

pub async fn list(db: &Db, f: &Filter) -> anyhow::Result<Vec<AuditRow>> {
    let rows = sqlx::query(
        "SELECT id, at, actor_kind, actor_run_id, action, entity_kind, entity_id, summary, detail
           FROM audit_log
          WHERE ($1::text IS NULL OR entity_kind = $1)
            AND ($2::text IS NULL OR entity_id = $2)
            AND ($3::text IS NULL OR actor_kind = $3)
            AND ($4::bigint IS NULL OR id < $4)
          ORDER BY id DESC LIMIT $5",
    )
    .bind(&f.entity_kind)
    .bind(&f.entity_id)
    .bind(&f.actor_kind)
    .bind(f.before)
    .bind(f.limit.clamp(1, 1000))
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| AuditRow {
            id: r.get("id"),
            at: r.get("at"),
            actor_kind: r.get("actor_kind"),
            actor_run_id: r.get("actor_run_id"),
            action: r.get("action"),
            entity_kind: r.get("entity_kind"),
            entity_id: r.get("entity_id"),
            summary: r.get("summary"),
            detail: r.get("detail"),
        })
        .collect())
}

/// One CSV field, quoted when it needs to be — and defused when a
/// spreadsheet would read it as a formula, since summaries quote agents.
pub fn csv_field(s: &str) -> String {
    let defused = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if defused.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", defused.replace('"', "\"\""))
    } else {
        defused
    }
}

pub fn csv(rows: &[AuditRow]) -> String {
    let mut out =
        String::from("id,at,actor_kind,actor_run_id,action,entity_kind,entity_id,summary\n");
    for r in rows {
        let fields = [
            r.id.to_string(),
            r.at.to_rfc3339(),
            r.actor_kind.clone(),
            r.actor_run_id.map(|u| u.to_string()).unwrap_or_default(),
            r.action.clone(),
            r.entity_kind.clone().unwrap_or_default(),
            r.entity_id.clone().unwrap_or_default(),
            r.summary.clone(),
        ];
        out.push_str(
            &fields
                .iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_is_one_short_line() {
        assert_eq!(clip("a\n  b"), "a b");
        assert!(clip(&"x".repeat(500)).chars().count() <= MAX_SUMMARY + 1);
    }

    #[test]
    fn a_csv_cell_cannot_become_a_formula_or_break_its_row() {
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    /// The one writer. Any other file that inserts, updates or deletes the
    /// ledger fails this — and this file may only delete, in `prune`.
    #[test]
    fn only_this_module_writes_the_ledger() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let mut stack = vec![root.to_path_buf()];
        let needles = [
            concat!("INSERT INTO ", "audit_log"),
            concat!("UPDATE ", "audit_log"),
            concat!("DELETE FROM ", "audit_log"),
        ];
        let mut offenders = vec![];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if !path.ends_with("target") {
                        stack.push(path);
                    }
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                let mine = path.ends_with("src/audit.rs");
                // This file's own tests age rows to test the prune; the rule
                // is about the code that ships.
                let shipped = if mine {
                    text.split("#[cfg(test)]").next().unwrap_or("")
                } else {
                    &text
                };
                let flat = shipped.split_whitespace().collect::<Vec<_>>().join(" ");
                for n in needles {
                    if !flat.contains(n) {
                        continue;
                    }
                    let allowed = mine && !n.starts_with("UPDATE");
                    if !allowed {
                        offenders.push(format!("{} writes the ledger with {n}", path.display()));
                    }
                }
            }
        }
        assert!(offenders.is_empty(), "{offenders:#?}");
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    /// What a person did through the API is kept; agents' and Eren's own
    /// high-volume rows age out. Filters and the cursor page as they say.
    #[tokio::test]
    async fn retention_keeps_what_people_did_and_ages_out_the_rest() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let run = Uuid::new_v4();
        record(
            &t.db,
            Entry::new(Actor::Api, "POST /api/tasks/{id}/merge")
                .on("tasks", "t1")
                .summary("merge"),
        )
        .await;
        record(
            &t.db,
            Entry::new(Actor::Agent(run), "tool comment").on("runs", run),
        )
        .await;
        record(&t.db, Entry::new(Actor::System, "routine fired (cron)")).await;
        sqlx::query("UPDATE audit_log SET at = now() - interval '91 days'")
            .execute(&t.db.pool)
            .await
            .unwrap();
        record(
            &t.db,
            Entry::new(Actor::System, "routine fired (cron)").summary("recent"),
        )
        .await;

        assert_eq!(
            prune(&t.db).await.unwrap(),
            2,
            "the old agent and system rows"
        );
        let left = list(
            &t.db,
            &Filter {
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let kinds: Vec<_> = left.iter().map(|r| r.actor_kind.as_str()).collect();
        assert_eq!(
            kinds,
            ["system", "api"],
            "newest first: the recent system row, the old api row"
        );

        let one_card = list(
            &t.db,
            &Filter {
                entity_kind: Some("tasks".into()),
                entity_id: Some("t1".into()),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(one_card.len(), 1);
        let page = list(
            &t.db,
            &Filter {
                before: Some(left[0].id),
                limit: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(page.len(), 1, "the cursor skips what was already shown");
        t.finish().await;
    }
}
