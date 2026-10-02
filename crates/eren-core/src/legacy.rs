//! Bringing state from before the rename across.
//!
//! Eren was called aichip. The names live in `eren_shared::brand`; this is
//! the part that needs a database: renaming the managed one, and giving the
//! apps Eren made their manifest's new name. Both run at boot, both are no-ops
//! the second time, and both go when the compatibility window closes —
//! which is why they are here together rather than spread across the modules
//! they touch.

use crate::Db;
use sqlx::Connection;

/// Rename a database from the name it had before the rename, once.
///
/// For the Postgres `eren serve` manages itself, whose database used to be
/// called `aichip`. Renamed in place rather than copied: it is the same data,
/// and `ALTER DATABASE … RENAME` is instant whatever its size. Done before the
/// pool opens, because Postgres refuses to rename a database anything is
/// connected to — and at boot nothing is.
///
/// `admin_url` names a database on the same server to connect from (one cannot
/// rename the database one is in). Returns whether anything was renamed: not
/// when `to` already exists, and not when `from` does not.
///
/// `from` and `to` are constants from `eren_shared::brand`, never input; they
/// are quoted all the same.
pub async fn adopt_database(admin_url: &str, from: &str, to: &str) -> anyhow::Result<bool> {
    let mut conn = sqlx::PgConnection::connect(admin_url).await?;
    let exists = |name: &str| {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)",
        )
        .bind(name.to_string())
    };
    let renamed = if exists(to).fetch_one(&mut conn).await? {
        false
    } else if exists(from).fetch_one(&mut conn).await? {
        let quote = |n: &str| format!("\"{}\"", n.replace('"', "\"\""));
        sqlx::query(&format!(
            "ALTER DATABASE {} RENAME TO {}",
            quote(from),
            quote(to)
        ))
        .execute(&mut conn)
        .await?;
        true
    } else {
        false
    };
    conn.close().await?;
    Ok(renamed)
}

/// Give every app made before the rename its manifest's new name.
///
/// An app's folder is Eren's own, so unlike a manifest committed to somebody's
/// repository (which `sync` only ever reads) this one is renamed — and
/// committed, so the next build's worktree opens on the same name the prompt
/// tells its agent to edit. Run at boot; a folder already renamed is skipped,
/// so a second boot does nothing. Returns how many were renamed.
pub async fn adopt_legacy_manifests(db: &Db) -> anyhow::Result<usize> {
    use eren_shared::brand::{APP_MANIFEST, LEGACY_APP_MANIFEST};
    let mut renamed = 0;
    for app in crate::apps::list(db, None).await? {
        let old = app.path.join(LEGACY_APP_MANIFEST);
        let new = app.path.join(APP_MANIFEST);
        if !old.is_file() || new.exists() {
            continue;
        }
        tokio::fs::rename(&old, &new).await?;
        crate::apps::commit(
            &app.path,
            &format!("Rename {LEGACY_APP_MANIFEST} to {APP_MANIFEST}"),
        )
        .await?;
        renamed += 1;
    }
    Ok(renamed)
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;
    use sqlx::Connection;

    #[tokio::test]
    async fn the_old_database_is_renamed_once_and_keeps_its_data() {
        // `fresh` makes a migrated database; this renames it the way boot
        // renames the managed one, and checks the rows came along.
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let admin = std::env::var("DATABASE_URL").unwrap();
        let from = t.name().to_string();
        let to = format!("{from}_renamed");
        sqlx::query("INSERT INTO workspaces (name) VALUES ('kept')")
            .execute(&t.db.pool)
            .await
            .unwrap();
        t.db.pool.close().await;

        assert!(adopt_database(&admin, &from, &to).await.unwrap());
        // The second boot finds the new name and leaves it.
        assert!(!adopt_database(&admin, &from, &to).await.unwrap());

        let url = testdb::with_database(&admin, &to);
        let mut conn = sqlx::PgConnection::connect(&url).await.unwrap();
        let kept: String = sqlx::query_scalar("SELECT name FROM workspaces WHERE name = 'kept'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(kept, "kept");
        conn.close().await.unwrap();

        let mut conn = sqlx::PgConnection::connect(&admin).await.unwrap();
        sqlx::query(&format!("DROP DATABASE IF EXISTS \"{to}\""))
            .execute(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        t.finish().await;
    }

    #[tokio::test]
    async fn an_agent_saved_before_the_rename_keeps_its_tools() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let ws: uuid::Uuid =
            sqlx::query_scalar("INSERT INTO workspaces (name) VALUES ('w') RETURNING id")
                .fetch_one(&t.db.pool)
                .await
                .unwrap();
        let before = [
            "Read",
            "mcp__aichip__create_task",
            "mcp__aichip",
            "mcp__aichipper__x",
            "mcp__playwright__click",
        ];
        let agent: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO agents (workspace_id, name, allowed_tools) VALUES ($1, 'a', $2)
             RETURNING id",
        )
        .bind(ws)
        .bind(&before[..])
        .fetch_one(&t.db.pool)
        .await
        .unwrap();

        // The migration has already run on this fresh database; run it again
        // over the row it would have found on an upgraded one.
        sqlx::raw_sql(include_str!("../migrations/0088_rename_tools.sql"))
            .execute(&t.db.pool)
            .await
            .unwrap();

        let after: Vec<String> =
            sqlx::query_scalar("SELECT allowed_tools FROM agents WHERE id = $1")
                .bind(agent)
                .fetch_one(&t.db.pool)
                .await
                .unwrap();
        assert_eq!(
            after,
            [
                "Read",
                "mcp__eren__create_task",
                "mcp__eren",
                // Another server that merely starts the same way is untouched.
                "mcp__aichipper__x",
                "mcp__playwright__click",
            ]
        );
        t.finish().await;
    }

    #[tokio::test]
    async fn nothing_to_rename_is_not_an_error() {
        let Ok(admin) = std::env::var("DATABASE_URL") else {
            return;
        };
        let ghost = format!("eren_test_{}", uuid::Uuid::new_v4().simple());
        assert!(!adopt_database(&admin, &ghost, &format!("{ghost}_x"))
            .await
            .unwrap());
    }
}
