//! Keep a project's code index in step with its files.
//!
//! On-demand, never a watcher (the repo's rule: poll or on-demand): called
//! when the project page opens, after a card's work lands, and when `HEAD`
//! moves. A sha256 hash-diff makes the repeat calls cheap — an unchanged file
//! costs one read and one hash, not a re-embed.
//!
//! Two phases, and they are separate on purpose. `structure` enumerates and
//! hashes: it finishes in seconds and needs no model, so the index is useful
//! offline and before the embedder has ever downloaded. `embedding` fills in
//! the vectors, which waits on a 35MB model the first time. Collapsing them
//! into one "indexing" state would make the UI claim the index is incomplete
//! long after the part that answers most questions is finished.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::db::Db;

/// What one pass did.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexReport {
    pub indexed: usize,
    pub unchanged: usize,
    pub failed: usize,
    pub removed: usize,
    /// True when the embedder was not ready, so the structure landed and the
    /// vectors did not. Not an error: the index still answers by path.
    pub vectors_deferred: bool,
}

/// One reconcile per project at a time.
///
/// The page-open trigger and the after-a-card-lands trigger will collide —
/// opening the board while a run finishes is the ordinary case — and two
/// interleaved delete-and-insert passes over one file corrupt its chunk set.
/// Same shape as `rag::index`, for the same reason.
static LOCKS: LazyLock<Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock_for(project_id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = LOCKS.lock().unwrap();
    locks.entry(project_id).or_default().clone()
}

/// Where the project is and whether git can enumerate it.
async fn project_root(db: &Db, project_id: Uuid) -> anyhow::Result<(PathBuf, bool)> {
    let row = sqlx::query("SELECT path, vcs FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&db.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no such project"))?;
    Ok((
        PathBuf::from(row.get::<String, _>("path")),
        row.get::<String, _>("vcs") == "git",
    ))
}

/// The commit the working tree is on, or `None` before the first one.
pub async fn head_sha(root: &Path) -> Option<String> {
    let out = tokio::process::Command::new("git")
        .current_dir(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Is the stored index still describing the tree as it stands?
///
/// Cheap enough to ask on every page open: one `git rev-parse`. It answers the
/// common case (nothing has moved) without reading a single file. It does not
/// catch uncommitted edits — the hash-diff does that, and only when something
/// asks for a full pass.
pub async fn is_stale(db: &Db, project_id: Uuid) -> bool {
    let Ok((root, is_git)) = project_root(db, project_id).await else {
        return false;
    };
    if !is_git {
        return true; // no commits to compare; the hash-diff decides
    }
    let stored: Option<String> = sqlx::query_scalar(
        "SELECT head_sha FROM project_index WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten();
    match (stored, head_sha(&root).await) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    }
}

async fn set_phase(db: &Db, project_id: Uuid, phase: &str) {
    let _ = sqlx::query(
        "INSERT INTO project_index (project_id, phase, updated_at)
         VALUES ($1, $2, now())
         ON CONFLICT (project_id) DO UPDATE SET phase = EXCLUDED.phase, updated_at = now()",
    )
    .bind(project_id)
    .bind(phase)
    .execute(&db.pool)
    .await;
}

/// Read the project, and embed whatever changed.
///
/// Never fatal to the caller's own work: every per-file failure is recorded on
/// that file's row and the pass continues, because one unreadable file must
/// not cost the project its index.
pub async fn reconcile(db: &Db, project_id: Uuid) -> anyhow::Result<IndexReport> {
    let lock = lock_for(project_id);
    let _guard = lock.lock().await;

    let (root, is_git) = project_root(db, project_id).await?;
    if !root.is_dir() {
        anyhow::bail!("this project's folder is not on disk");
    }
    set_phase(db, project_id, "structure").await;

    let mut report = IndexReport::default();
    let paths = super::enumerate::files(&root, is_git).await?;

    // What the database believes, so an unchanged file costs a hash and not an
    // embed. A `failed` row is deliberately not "known": it retries next pass.
    let known: HashMap<String, (Uuid, String, String)> = sqlx::query(
        "SELECT id, rel_path, content_hash, status FROM project_documents WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_all(&db.pool)
    .await?
    .into_iter()
    .map(|r| {
        (
            r.get::<String, _>("rel_path"),
            (
                r.get::<Uuid, _>("id"),
                r.get::<String, _>("content_hash"),
                r.get::<String, _>("status"),
            ),
        )
    })
    .collect();

    let mut changed: Vec<(String, String, Vec<super::chunk::CodeChunk>, u64)> = vec![];
    for rel in &paths {
        let full = root.join(rel);
        let Ok(meta) = tokio::fs::metadata(&full).await else {
            continue; // vanished between enumeration and now
        };
        if meta.len() > super::enumerate::MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = tokio::fs::read(&full).await else {
            report.failed += 1;
            continue;
        };
        let hash = hex::encode(Sha256::digest(&bytes));
        if let Some((_, known_hash, status)) = known.get(rel) {
            if known_hash == &hash && status == "indexed" {
                report.unchanged += 1;
                continue;
            }
        }
        // Binary or otherwise unreadable as text: skipped, not failed. The
        // agent can still open it; it just is not searchable by meaning.
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let chunks = super::chunk::chunk_code(&text);
        if chunks.is_empty() {
            continue;
        }
        changed.push((rel.clone(), hash, chunks, meta.len()));
    }

    // Structure is done the moment we know what changed. Say so before the
    // slow half starts, so the UI can stop claiming it is still reading.
    let _ = sqlx::query(
        "INSERT INTO project_index (project_id, phase, head_sha, files_total, files_parsed,
                                    structure_version, indexed_at, error, note, updated_at)
         VALUES ($1, $2, $3, $4, $4, 1, now(), NULL, $5, now())
         ON CONFLICT (project_id) DO UPDATE SET
             phase = EXCLUDED.phase,
             head_sha = EXCLUDED.head_sha,
             files_total = EXCLUDED.files_total,
             files_parsed = EXCLUDED.files_parsed,
             structure_version = project_index.structure_version
                 + CASE WHEN $6 THEN 1 ELSE 0 END,
             indexed_at = now(),
             error = NULL,
             note = EXCLUDED.note,
             updated_at = now()",
    )
    .bind(project_id)
    .bind(if changed.is_empty() { "ready" } else { "embedding" })
    .bind(head_sha(&root).await)
    .bind(paths.len() as i32)
    .bind(paths.is_empty().then_some(
        "Nothing here in a language this understands yet — the map fills in when there is code to read.",
    ))
    .bind(!changed.is_empty())
    .execute(&db.pool)
    .await;

    // The embedder is *not* pre-checked, and that is load-bearing: the model
    // downloads lazily on the first `embed_batch`, so gating on `status()`
    // being Ready meant nothing ever asked, the download never started, and
    // the vectors phase waited forever on a condition only it could satisfy.
    // Ask, and let the failure be the answer.
    for (rel, hash, chunks, bytes) in changed {
        match embed_one(db, project_id, &rel, &hash, bytes, chunks).await {
            Ok(()) => report.indexed += 1,
            Err(e) => {
                // A model that will not load fails every file the same way, so
                // one report line beats 194 identical rows. The structure
                // still stands and search by path still works.
                report.vectors_deferred = true;
                report.failed += 1;
                let _ = sqlx::query(
                    "INSERT INTO project_documents (project_id, rel_path, content_hash, bytes, status, error)
                     VALUES ($1,$2,$3,$4,'failed',$5)
                     ON CONFLICT (project_id, rel_path) DO UPDATE SET
                         status='failed', error=EXCLUDED.error, content_hash=EXCLUDED.content_hash",
                )
                .bind(project_id)
                .bind(&rel)
                .bind(&hash)
                .bind(bytes as i64)
                .bind(e.to_string())
                .execute(&db.pool)
                .await;
            }
        }
    }

    finish(db, project_id, root, paths, report).await
}

/// One file's chunks, replaced wholesale in a single transaction.
///
/// The old chunk set and the new one must never both be visible: a query
/// landing between the delete and the insert would rank a file's previous
/// contents against its current name.
async fn embed_one(
    db: &Db,
    project_id: Uuid,
    rel: &str,
    hash: &str,
    bytes: u64,
    chunks: Vec<super::chunk::CodeChunk>,
) -> anyhow::Result<()> {
    let vectors =
        crate::rag::embed::embed_batch(chunks.iter().map(|c| c.content.clone()).collect()).await?;
    if vectors.len() != chunks.len() {
        anyhow::bail!("the embedder returned {} vectors for {} chunks", vectors.len(), chunks.len());
    }

    let mut tx = db.pool.begin().await?;
    let doc_id: Uuid = sqlx::query_scalar(
        "INSERT INTO project_documents (project_id, rel_path, content_hash, bytes, status, error, indexed_at)
         VALUES ($1,$2,$3,$4,'indexed',NULL,now())
         ON CONFLICT (project_id, rel_path) DO UPDATE SET
             content_hash = EXCLUDED.content_hash, bytes = EXCLUDED.bytes,
             status='indexed', error=NULL, indexed_at=now()
         RETURNING id",
    )
    .bind(project_id)
    .bind(rel)
    .bind(hash)
    .bind(bytes as i64)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM project_chunks WHERE document_id = $1")
        .bind(doc_id)
        .execute(&mut *tx)
        .await?;

    for (i, (chunk, vector)) in chunks.iter().zip(vectors.iter()).enumerate() {
        sqlx::query(
            "INSERT INTO project_chunks
                 (document_id, project_id, chunk_index, content, embedding, embedding_model,
                  start_line, symbol)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(doc_id)
        .bind(project_id)
        .bind(i as i32)
        .bind(&chunk.content)
        .bind(crate::rag::embed::to_bytes(vector))
        .bind(crate::rag::embed::MODEL_TAG)
        .bind(chunk.start_line)
        .bind(chunk.symbol.as_deref())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Drop what is no longer on disk, and settle the phase.
async fn finish(
    db: &Db,
    project_id: Uuid,
    root: PathBuf,
    paths: Vec<String>,
    mut report: IndexReport,
) -> anyhow::Result<IndexReport> {
    // A file deleted from the repository must stop being findable. A map that
    // names a path which no longer exists sends an agent to read a ghost.
    let removed = sqlx::query(
        "DELETE FROM project_documents WHERE project_id = $1 AND NOT (rel_path = ANY($2))",
    )
    .bind(project_id)
    .bind(&paths)
    .execute(&db.pool)
    .await?
    .rows_affected();
    report.removed = removed as usize;

    let embedded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project_documents WHERE project_id = $1 AND status = 'indexed'",
    )
    .bind(project_id)
    .fetch_one(&db.pool)
    .await
    .unwrap_or(0);

    let _ = sqlx::query(
        "UPDATE project_index
            SET phase = $2, files_embedded = $3, head_sha = $4, updated_at = now()
          WHERE project_id = $1",
    )
    .bind(project_id)
    .bind(if report.vectors_deferred { "ready" } else { "ready" })
    .bind(embedded as i32)
    .bind(head_sha(&root).await)
    .execute(&db.pool)
    .await;
    Ok(report)
}
