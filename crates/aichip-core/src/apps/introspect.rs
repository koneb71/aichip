//! What an app's schema actually looks like right now.
//!
//! Read from `information_schema` and `pg_indexes` every time, never cached and
//! never mirrored into a table of our own. A registry of "what we think the
//! schema is" drifts the moment anything touches a table outside the code that
//! maintains the registry — and then every diff is computed against a fiction,
//! which is worse than having no diff at all. Postgres already knows.

use super::schema::{LiveColumn, LiveTable};
use crate::Db;
use sqlx::Row;

/// Every table in an app's schema, with its columns and indexes.
///
/// An absent schema is an empty list, not an error: an app whose tables have
/// never been created is the ordinary state at install, and the plan against an
/// empty list is exactly "create everything".
pub async fn tables(db: &Db, schema: &str) -> anyhow::Result<Vec<LiveTable>> {
    let rows = sqlx::query(
        "SELECT table_name, column_name, data_type
           FROM information_schema.columns
          WHERE table_schema = $1
          ORDER BY table_name, ordinal_position",
    )
    .bind(schema)
    .fetch_all(&db.pool)
    .await?;

    let mut out: Vec<LiveTable> = Vec::new();
    for row in &rows {
        let table: String = row.get("table_name");
        let column = LiveColumn {
            name: row.get("column_name"),
            data_type: row.get("data_type"),
        };
        match out.iter_mut().find(|t| t.name == table) {
            Some(t) => t.columns.push(column),
            None => out.push(LiveTable { name: table, columns: vec![column], indexes: vec![] }),
        }
    }

    // Indexes are a separate catalogue. The primary key's own index is skipped:
    // it is not something a manifest asks for, so leaving it in would make the
    // planner propose dropping it on every single build.
    let idx = sqlx::query(
        "SELECT tablename, indexname FROM pg_indexes
          WHERE schemaname = $1 AND indexname LIKE '%\\_idx'",
    )
    .bind(schema)
    .fetch_all(&db.pool)
    .await?;
    for row in &idx {
        let table: String = row.get("tablename");
        if let Some(t) = out.iter_mut().find(|t| t.name == table) {
            t.indexes.push(row.get("indexname"));
        }
    }

    Ok(out)
}
