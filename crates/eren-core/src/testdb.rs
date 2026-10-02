//! A throwaway database per test, for the tests that need Postgres.
//!
//! Most of Eren's tests are pure, and the ones about a run's life drive the
//! mock engine — but the parts that matter most for a card's life (who may
//! start, what landing wakes, what a run leaves on its card) are SQL, and a
//! pure test of SQL tests nothing. These run against a real database.
//!
//! Each test gets a database of its own (`eren_test_<random>`), migrated
//! from scratch, created on the server `DATABASE_URL` names and dropped when
//! the test finishes. Only databases this module created are ever dropped —
//! never the one the URL points at. With no `DATABASE_URL` the tests skip
//! (saying so), so `cargo test` still works on a machine with no Postgres;
//! CI provides one.

use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use crate::bus::EventBus;
use crate::db::Db;
use crate::runs::orchestrator::Orchestrator;
use crate::worktrees::manager::WorktreeManager;

pub(crate) struct TestDb {
    pub db: Db,
    admin_url: String,
    name: String,
}

/// A fresh, migrated database — or `None`, with a note, when there is no
/// server to make one on.
pub(crate) async fn fresh() -> Option<TestDb> {
    let Ok(url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipped: set DATABASE_URL to run the database tests");
        return None;
    };
    let name = format!("eren_test_{}", Uuid::new_v4().simple());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("DATABASE_URL is set but the server cannot be reached");
    // `name` is generated above, never input.
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .expect("could not create a test database — does the role have CREATEDB?");
    admin.close().await;
    let db = Db::connect(&with_database(&url, &name))
        .await
        .expect("migrations failed on a fresh database");
    Some(TestDb {
        db,
        admin_url: url,
        name,
    })
}

impl TestDb {
    /// Drop the database. Called at the end of a test; one that panics first
    /// leaves its database behind, named so it is obvious what it was.
    pub async fn finish(self) {
        self.db.pool.close().await;
        if let Ok(admin) = PgPoolOptions::new()
            .max_connections(1)
            .connect(&self.admin_url)
            .await
        {
            let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS {}", self.name))
                .execute(&admin)
                .await;
        }
    }

    /// An orchestrator over this database with only the mock engine, its
    /// dispatch loop running.
    pub fn orchestrator(&self, worktrees_root: &std::path::Path) -> Arc<Orchestrator> {
        let mut orchestrator = Orchestrator::new(
            self.db.clone(),
            EventBus::new(),
            Arc::new(WorktreeManager::new(worktrees_root.to_path_buf())),
            4,
            None,
        );
        orchestrator.register_engine(Arc::new(eren_engines::mock::MockEngine::demo()));
        let orchestrator = Arc::new(orchestrator);
        tokio::spawn(orchestrator.clone().run_loop());
        orchestrator
    }

    /// A workspace and a project in it. `in_place` makes a project with no
    /// version control, whose runs work in the folder and settle to done.
    pub async fn project(&self, path: &std::path::Path, in_place: bool) -> (Uuid, Uuid) {
        let ws: Uuid =
            sqlx::query_scalar("INSERT INTO workspaces (name) VALUES ('test') RETURNING id")
                .fetch_one(&self.db.pool)
                .await
                .unwrap();
        let project: Uuid = sqlx::query_scalar(
            "INSERT INTO projects (path, name, workspace_id, vcs)
             VALUES ($1, 'test', $2, $3) RETURNING id",
        )
        .bind(path.to_string_lossy().as_ref())
        .bind(ws)
        .bind(if in_place { "none" } else { "git" })
        .fetch_one(&self.db.pool)
        .await
        .unwrap();
        (ws, project)
    }

    /// A backlog card on the mock engine.
    pub async fn card(&self, project: Uuid, title: &str) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO tasks (project_id, title, prompt, engine, board_column)
             VALUES ($1, $2, $2, 'mock', 'backlog') RETURNING id",
        )
        .bind(project)
        .bind(title)
        .fetch_one(&self.db.pool)
        .await
        .unwrap()
    }

    /// Wait until `sql` (a boolean query, `$1` bound to `id`) holds, or fail
    /// after ten seconds saying what it was waiting for.
    pub async fn until(&self, what: &str, sql: &str, id: Uuid) {
        for _ in 0..100 {
            let ok: bool = sqlx::query_scalar(sql)
                .bind(id)
                .fetch_one(&self.db.pool)
                .await
                .unwrap_or(false);
            if ok {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("timed out waiting for {what}");
    }
}

/// The same server and credentials, another database.
fn with_database(url: &str, name: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    // postgres://user:pass@host:port/db — the database is the last segment
    // after the authority; a URL with no path gets one.
    let after_scheme = base.find("://").map(|i| i + 3).unwrap_or(0);
    let base = match base[after_scheme..].find('/') {
        Some(slash) => &base[..after_scheme + slash],
        None => base,
    };
    match query {
        Some(q) => format!("{base}/{name}?{q}"),
        None => format!("{base}/{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::with_database;

    #[test]
    fn only_the_database_name_changes() {
        assert_eq!(
            with_database("postgres://a:b@h:5433/eren", "t1"),
            "postgres://a:b@h:5433/t1"
        );
        assert_eq!(
            with_database("postgres://a@h/eren?sslmode=disable", "t1"),
            "postgres://a@h/t1?sslmode=disable"
        );
        assert_eq!(
            with_database("postgres://h:5432", "t1"),
            "postgres://h:5432/t1"
        );
    }
}
