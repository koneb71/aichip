//! The page's memory: an append-only log, and the rule that a body is never
//! written directly.
//!
//! The live body of a page is the newest `accepted` revision. A person's save
//! is accepted immediately; an agent's is only ever **proposed**, and a human
//! accepts or discards it. That single asymmetry is what makes it safe for an
//! agent to have write access to a wiki a person also edits — before it, one
//! documentation run silently replaced a page someone had carefully written,
//! with no copy, no diff and no trace.
//!
//! Three invariants hold the whole thing up:
//!
//! 1. `kb_articles.current_seq` equals the highest **accepted** seq, and never
//!    decreases.
//! 2. Every write allocates its seq while holding a row lock on the article,
//!    so two concurrent saves cannot mint the same one.
//! 3. `kb_articles.body_version` advances on every accepted body write, and it
//!    — not `current_seq` — is the optimistic-concurrency token. See
//!    [`save_edit`] for why the two cannot be the same number.

use crate::db::Db;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

/// Who wrote a revision. There is no users table; this is all the authorship
/// that exists, and it is what the review UI keys on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Author {
    Human,
    Agent,
}

impl Author {
    pub fn as_str(self) -> &'static str {
        match self {
            Author::Human => "human",
            Author::Agent => "agent",
        }
    }
}

/// A revision as the history view shows it.
#[derive(Debug, Clone)]
pub struct Revision {
    pub seq: i32,
    pub kind: String,
    pub state: String,
    pub author_kind: String,
    pub title: String,
    pub base_seq: Option<i32>,
    pub restored_from: Option<i32>,
    pub run_id: Option<Uuid>,
    pub note: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Characters added and removed against the previous accepted revision.
    pub chars: i64,
}

/// What a caller is asking to write.
pub struct NewRevision<'a> {
    pub title: &'a str,
    pub html: &'a str,
    pub text: &'a str,
    pub author: Author,
    pub kind: &'a str,
    pub base_seq: Option<i32>,
    pub run_id: Option<Uuid>,
    pub note: &'a str,
}

/// Two human saves inside this window collapse into one revision.
///
/// The editor autosaves on a debounce, so without this a ten-minute writing
/// session leaves eighty rows and the history view becomes unreadable — which
/// is the same as having no history.
///
/// Note what this does *not* check: which person is saving. There is no users
/// table, so every human save looks alike, and two different people editing the
/// same page within five minutes would collapse into one row. What keeps that
/// from destroying anything is the `base_version` guard in [`save_edit`] — the
/// second saver is refused before ever reaching this query. Weakening that
/// guard re-opens data loss here, not merely a messy history.
const COALESCE_SECONDS: i64 = 300;

/// Somebody else changed the page since this edit began.
#[derive(Debug)]
pub struct Conflict {
    pub current_seq: i32,
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "this page has changed since you started editing (it is now at revision {})",
            self.current_seq
        )
    }
}
impl std::error::Error for Conflict {}

/// Write a human edit: accepted immediately, and it becomes the live body.
///
/// `base_version` is `kb_articles.body_version` as the editor loaded it. When it
/// no longer matches, this refuses rather than overwriting — the caller shows a
/// diff instead. Pass `None` only where there is genuinely nothing to race with
/// (the boot-time import).
///
/// **It is deliberately not `base_seq`, and that distinction is the whole point
/// of the column.** `current_seq` looks like a version number and behaves like
/// one right up until a save coalesces, at which point the revision is rewritten
/// in place and the pointer stays put. Two editors that both loaded revision 5
/// therefore both still matched 5 after the first one saved: the second was
/// waved through, replaced the first person's text, and left no second row to
/// recover it from. `body_version` advances on every accepted write, coalesced
/// or not, so the second saver is refused.
///
/// `rev.base_seq` survives, but only as the diff anchor — which revision this
/// one is a change *against*. It no longer guards anything.
pub async fn save_edit(
    db: &Db,
    article_id: Uuid,
    rev: NewRevision<'_>,
    base_version: Option<i64>,
) -> anyhow::Result<i32> {
    let mut tx = db.pool.begin().await?;
    let head = lock_head(&mut tx, article_id).await?;
    let current = head.seq;
    if let Some(base) = base_version {
        if base != head.version {
            return Err(Conflict {
                current_seq: current,
            }
            .into());
        }
    }

    // Coalesce a rapid series of autosaves into the revision they are
    // extending, rather than one row per keystroke burst.
    let extendable: Option<i32> = sqlx::query_scalar(
        "SELECT seq FROM kb_revisions
         WHERE article_id = $1 AND seq = $2 AND state = 'accepted'
           AND author_kind = 'human' AND kind = 'edit'
           AND created_at > now() - make_interval(secs => $3)",
    )
    .bind(article_id)
    .bind(current)
    .bind(COALESCE_SECONDS as f64)
    .fetch_optional(&mut *tx)
    .await?;

    let seq = match extendable {
        Some(seq) => {
            sqlx::query(
                "UPDATE kb_revisions SET title=$3, content_html=$4, content_text=$5
                 WHERE article_id=$1 AND seq=$2",
            )
            .bind(article_id)
            .bind(seq)
            .bind(rev.title)
            .bind(rev.html)
            .bind(rev.text)
            .execute(&mut *tx)
            .await?;
            seq
        }
        None => insert(&mut tx, article_id, current + 1, &rev, "accepted").await?,
    };

    apply(
        &mut tx, article_id, seq, rev.title, rev.html, rev.text, rev.author,
    )
    .await?;
    tx.commit().await?;
    Ok(seq)
}

/// Record an agent's work as a proposal. Nothing about the live page changes.
pub async fn propose(db: &Db, article_id: Uuid, rev: NewRevision<'_>) -> anyhow::Result<i32> {
    let mut tx = db.pool.begin().await?;
    let current = lock_head(&mut tx, article_id).await?.seq;
    // Any earlier pending proposal for this page is superseded rather than
    // deleted: two banners stacked up would make the page unreviewable, and
    // the trail should still show that a proposal existed.
    sqlx::query(
        "UPDATE kb_revisions SET state='superseded', decided_at=now()
         WHERE article_id=$1 AND state='pending'",
    )
    .bind(article_id)
    .execute(&mut *tx)
    .await?;

    let seq = insert(&mut tx, article_id, current + 1, &rev, "pending").await?;
    tx.commit().await?;
    Ok(seq)
}

/// Accept a proposal, making it the live body.
///
/// The subtle case is a stale one: if a person saved after the agent started,
/// the proposal is no longer the highest seq, and promoting it in place would
/// erase that person's edit. So a stale proposal is *copied* to the top of the
/// log and the original marked superseded — `current_seq` only ever moves
/// forward, and the trail says exactly what happened.
pub async fn accept(db: &Db, article_id: Uuid, seq: i32) -> anyhow::Result<i32> {
    let mut tx = db.pool.begin().await?;
    let current = lock_head(&mut tx, article_id).await?.seq;

    // Locked, not merely read: without the row lock a concurrent discard
    // between this SELECT and the UPDATE below would be silently undone and
    // the rejected body would go live.
    let row = sqlx::query(
        "SELECT title, content_html, content_text, author_kind, state
         FROM kb_revisions WHERE article_id=$1 AND seq=$2 FOR UPDATE",
    )
    .bind(article_id)
    .bind(seq)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| anyhow::anyhow!("no such revision"))?;
    if row.get::<String, _>("state") != "pending" {
        anyhow::bail!("that revision is not waiting for a decision");
    }

    let title: String = row.get("title");
    let html: String = row.get("content_html");
    let text: String = row.get("content_text");
    let author = match row.get::<String, _>("author_kind").as_str() {
        "agent" => Author::Agent,
        _ => Author::Human,
    };

    let live = if seq > current {
        sqlx::query(
            "UPDATE kb_revisions SET state='accepted', decided_at=now()
             WHERE article_id=$1 AND seq=$2",
        )
        .bind(article_id)
        .bind(seq)
        .execute(&mut *tx)
        .await?;
        seq
    } else {
        sqlx::query(
            "UPDATE kb_revisions SET state='superseded', decided_at=now()
             WHERE article_id=$1 AND seq=$2",
        )
        .bind(article_id)
        .bind(seq)
        .execute(&mut *tx)
        .await?;
        let rev = NewRevision {
            title: &title,
            html: &html,
            text: &text,
            author,
            kind: "agent",
            // Against what it actually replaced, not against itself: pointing
            // this at the copied revision made the entry that changed the page
            // diff to nothing at all in the history view.
            base_seq: Some(current),
            run_id: sqlx::query_scalar(
                "SELECT run_id FROM kb_revisions WHERE article_id=$1 AND seq=$2",
            )
            .bind(article_id)
            .bind(seq)
            .fetch_optional(&mut *tx)
            .await?
            .flatten(),
            note: "accepted after the page had moved on",
        };
        insert(&mut tx, article_id, current + 1, &rev, "accepted").await?
    };

    apply(&mut tx, article_id, live, &title, &html, &text, author).await?;
    tx.commit().await?;
    Ok(live)
}

/// Turn a proposal down. The row stays — a trail with holes in it is not a
/// trail — it just stops being a candidate.
pub async fn discard(db: &Db, article_id: Uuid, seq: i32, note: &str) -> anyhow::Result<()> {
    let done = sqlx::query(
        "UPDATE kb_revisions SET state='discarded', decided_at=now(), note=$3
         WHERE article_id=$1 AND seq=$2 AND state='pending'",
    )
    .bind(article_id)
    .bind(seq)
    .bind(note)
    .execute(&db.pool)
    .await?;
    if done.rows_affected() == 0 {
        anyhow::bail!("that revision is not waiting for a decision");
    }
    Ok(())
}

/// Put an old revision back.
///
/// A forward write, never a rewind: restoring revision 3 appends a new one
/// carrying its content. History is a record of what happened, and rewinding
/// the pointer would make it a record of what someone wishes had happened.
pub async fn restore(db: &Db, article_id: Uuid, seq: i32) -> anyhow::Result<i32> {
    let mut tx = db.pool.begin().await?;
    let current = lock_head(&mut tx, article_id).await?.seq;
    let row = sqlx::query(
        "SELECT title, content_html, content_text FROM kb_revisions
         WHERE article_id=$1 AND seq=$2",
    )
    .bind(article_id)
    .bind(seq)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| anyhow::anyhow!("no such revision"))?;

    let title: String = row.get("title");
    let html: String = row.get("content_html");
    let text: String = row.get("content_text");
    let rev = NewRevision {
        title: &title,
        html: &html,
        text: &text,
        author: Author::Human,
        kind: "restore",
        base_seq: Some(current),
        run_id: None,
        note: &format!("restored from revision {seq}"),
    };
    let new_seq = insert(&mut tx, article_id, current + 1, &rev, "accepted").await?;
    sqlx::query("UPDATE kb_revisions SET restored_from=$3 WHERE article_id=$1 AND seq=$2")
        .bind(article_id)
        .bind(new_seq)
        .bind(seq)
        .execute(&mut *tx)
        .await?;
    apply(
        &mut tx,
        article_id,
        new_seq,
        &title,
        &html,
        &text,
        Author::Human,
    )
    .await?;
    tx.commit().await?;
    Ok(new_seq)
}

/// The proposal waiting on a person, if there is one.
pub async fn pending(db: &Db, article_id: Uuid) -> anyhow::Result<Option<Revision>> {
    let row = sqlx::query(
        "SELECT r.*, length(r.content_text) AS chars FROM kb_revisions r
         WHERE r.article_id=$1 AND r.state='pending' ORDER BY r.seq DESC LIMIT 1",
    )
    .bind(article_id)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(|r| to_revision(&r)))
}

pub async fn list(db: &Db, article_id: Uuid) -> anyhow::Result<Vec<Revision>> {
    let rows = sqlx::query(
        "SELECT r.*, length(r.content_text) AS chars FROM kb_revisions r
         WHERE r.article_id=$1 ORDER BY r.seq DESC LIMIT 200",
    )
    .bind(article_id)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows.iter().map(to_revision).collect())
}

/// One revision's text, for diffing.
pub async fn text_of(db: &Db, article_id: Uuid, seq: i32) -> anyhow::Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT content_text FROM kb_revisions WHERE article_id=$1 AND seq=$2")
            .bind(article_id)
            .bind(seq)
            .fetch_optional(&db.pool)
            .await?,
    )
}

pub async fn html_of(db: &Db, article_id: Uuid, seq: i32) -> anyhow::Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT content_html FROM kb_revisions WHERE article_id=$1 AND seq=$2")
            .bind(article_id)
            .bind(seq)
            .fetch_optional(&db.pool)
            .await?,
    )
}

fn to_revision(r: &sqlx::postgres::PgRow) -> Revision {
    Revision {
        seq: r.get("seq"),
        kind: r.get("kind"),
        state: r.get("state"),
        author_kind: r.get("author_kind"),
        title: r.get("title"),
        base_seq: r.get("base_seq"),
        restored_from: r.get("restored_from"),
        run_id: r.get("run_id"),
        note: r.get("note"),
        created_at: r.get("created_at"),
        chars: r.get::<Option<i32>, _>("chars").unwrap_or(0) as i64,
    }
}

/// Where the page stands: the live revision, and the token that guards it.
struct Head {
    seq: i32,
    version: i64,
}

/// Take the article's row lock and read the live pointer under it.
///
/// Every seq is allocated through here. Without the lock two concurrent saves
/// both read the same `current_seq`, both write `current_seq + 1`, and one
/// loses to a primary-key violation — or worse, to a silent overwrite.
async fn lock_head(tx: &mut Transaction<'_, Postgres>, article_id: Uuid) -> anyhow::Result<Head> {
    let row =
        sqlx::query("SELECT current_seq, body_version FROM kb_articles WHERE id=$1 FOR UPDATE")
            .bind(article_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no such page"))?;
    Ok(Head {
        seq: row.get("current_seq"),
        version: row.get("body_version"),
    })
}

async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    article_id: Uuid,
    seq: i32,
    rev: &NewRevision<'_>,
    state: &str,
) -> anyhow::Result<i32> {
    // Not `current_seq + 1` blindly: a discarded or superseded proposal
    // already occupies a seq above it, and the primary key would collide.
    let seq: i32 = sqlx::query_scalar(
        "SELECT GREATEST($2, COALESCE(MAX(seq), 0) + 1) FROM kb_revisions WHERE article_id=$1",
    )
    .bind(article_id)
    .bind(seq)
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query(
        "INSERT INTO kb_revisions
            (article_id, seq, kind, state, author_kind, title, content_html, content_text,
             base_seq, run_id, note, decided_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,
                 CASE WHEN $4 = 'pending' THEN NULL ELSE now() END)",
    )
    .bind(article_id)
    .bind(seq)
    .bind(rev.kind)
    .bind(state)
    .bind(rev.author.as_str())
    .bind(rev.title)
    .bind(rev.html)
    .bind(rev.text)
    .bind(rev.base_seq)
    .bind(rev.run_id)
    .bind(rev.note)
    .execute(&mut **tx)
    .await?;
    Ok(seq)
}

/// Make a revision the live body.
///
/// The single place `body_version` moves, which is why it can be trusted as the
/// concurrency token: every path that changes what a reader sees — a save, a
/// coalesced autosave, an accepted proposal, a restore — comes through here, and
/// none of them can advance the body without advancing the token with it.
async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    article_id: Uuid,
    seq: i32,
    title: &str,
    html: &str,
    text: &str,
    author: Author,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE kb_articles
            SET title=$2, content_html=$3, content_text=$4, summary=$5,
                origin=$6, current_seq=$7, body_version=body_version+1,
                updated_at=now()
          WHERE id=$1",
    )
    .bind(article_id)
    .bind(title)
    .bind(html)
    .bind(text)
    .bind(super::sanitize::summarize(html, 200))
    // Provenance follows whoever wrote the version now live, rather than
    // sticking permanently to the first agent that ever touched the page.
    // Deliberately does NOT touch `status`: an agent's typo fix must not pull
    // a published page offline, and accepting is exactly the human review that
    // `published` is supposed to attest to.
    .bind(author.as_str())
    .bind(seq)
    .execute(&mut **tx)
    .await?;

    rebuild_links(tx, article_id, html).await
}

/// Rewrite this page's outgoing links from the body that is now live.
async fn rebuild_links(
    tx: &mut Transaction<'_, Postgres>,
    article_id: Uuid,
    html: &str,
) -> anyhow::Result<()> {
    let targets = super::render::prepare(html).link_ids;
    sqlx::query("DELETE FROM kb_links WHERE from_id=$1")
        .bind(article_id)
        .execute(&mut **tx)
        .await?;
    if targets.is_empty() {
        return Ok(());
    }
    // A link to a page that has since been deleted is dropped rather than
    // failing the save — the body keeps the dead href, which is honest, and
    // the backlink graph stays referentially sound.
    sqlx::query(
        "INSERT INTO kb_links (from_id, to_id)
         SELECT $1, id FROM kb_articles WHERE id = ANY($2) AND id <> $1
         ON CONFLICT DO NOTHING",
    )
    .bind(article_id)
    .bind(&targets)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
