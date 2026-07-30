//! Bring pages written before the wiki existed up to the new shape.
//!
//! Two things cannot be done in SQL. `content_text` needs the HTML stripper,
//! which only exists in Rust — a `regexp_replace('<[^>]*>')` would mangle
//! `<pre>` and skip entities, so the search index would be built from a
//! *different* projection than every subsequent write, and searching would
//! work differently for old and new pages with nothing to indicate why. And
//! the first revision needs that same projection to be diffable.
//!
//! It also re-sanitises every stored body. Sanitising happens on write, so a
//! body stored under older, looser rules keeps whatever it had until someone
//! happens to re-save it; this is the one pass that catches those.
//!
//! Idempotent, and it runs before the server binds — a half-migrated wiki that
//! serves requests is a wiki where some pages are unsearchable and nothing
//! says so.

use crate::db::Db;
use sqlx::Row;
use uuid::Uuid;

/// Selecting on `current_seq = 0` alone was not idempotent: a body made of
/// nothing but markup projects to empty text, `apply` then writes
/// `content_text = ''`, and the row matched again on the next boot — a fresh
/// "import" revision every time the server started. The revision log is the
/// honest test for "has this page been imported".
pub async fn run(db: &Db) -> anyhow::Result<usize> {
    let rows = sqlx::query(
        "SELECT id, title, content_html FROM kb_articles
          WHERE current_seq = 0
            AND NOT EXISTS (SELECT 1 FROM kb_revisions r WHERE r.article_id = kb_articles.id)",
    )
    .fetch_all(&db.pool)
    .await?;
    if rows.is_empty() {
        return Ok(0);
    }

    let mut done = 0;
    for row in &rows {
        let id: Uuid = row.get("id");
        let title: String = row.get("title");
        let html: String = row.get("content_html");
        // A page an agent is still writing has no body yet; leaving it at
        // current_seq = 0 is exactly right, and inventing an empty revision 1
        // for it would put a blank entry at the top of its history.
        if html.trim().is_empty() {
            continue;
        }
        if let Err(e) = one(db, id, &title, &html).await {
            // One unconvertible page must not stop the rest from becoming
            // searchable, but it has to be visible rather than swallowed.
            tracing::error!(%id, error = %e, "could not migrate knowledge-base page");
            continue;
        }
        done += 1;
    }
    tracing::info!(pages = done, "knowledge base migrated to revisions");
    Ok(done)
}

async fn one(db: &Db, id: Uuid, title: &str, html: &str) -> anyhow::Result<()> {
    let prepared = super::render::prepare(html);
    let rev = super::revisions::NewRevision {
        title,
        html: &prepared.html,
        text: &prepared.text,
        author: super::revisions::Author::Human,
        kind: "import",
        base_seq: None,
        run_id: None,
        note: "the page as it stood before history was kept",
    };
    // `save_edit` unguarded: this runs at boot, before the server accepts a
    // request, so there is no concurrent editor to conflict with. It writes
    // content_text, summary, the current_seq pointer and the backlinks in one
    // transaction.
    super::revisions::save_edit(db, id, rev, None).await?;
    Ok(())
}
