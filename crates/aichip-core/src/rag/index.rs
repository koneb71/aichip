//! Keep a space's semantic index in step with its folder.
//!
//! On-demand, never a watcher (the repo's rule: poll or on-demand): called
//! after an upload, from the Reindex button, and when the documents panel
//! loads. A sha256 hash-diff makes the repeat calls cheap — an unchanged
//! file costs one hash, not a re-embed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::db::Db;

/// Files bigger than this are `unsupported` for indexing. Ten megabytes
/// matches the upload cap and covers real PDFs and decks; the extracted
/// *text* has its own ceiling in `extract::MAX_EXTRACT_CHARS`. The agent's
/// own Read still opens oversize files in-folder.
pub const MAX_INDEX_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileReport {
    pub indexed: usize,
    pub failed: usize,
    pub unsupported: usize,
    pub removed: usize,
    pub unchanged: usize,
}

/// One reconcile per space at a time. Upload-then-panel-poll makes concurrent
/// calls a certainty, and two interleaved delete+insert passes over the same
/// document corrupt its chunk set.
static LOCKS: LazyLock<Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock_for(project_id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
    LOCKS.lock().unwrap().entry(project_id).or_default().clone()
}

/// Walk the space's folder and bring `project_documents`/`project_chunks` in
/// line with it: new and changed files are chunked and embedded, missing
/// files lose their rows, unchanged files cost a hash.
pub async fn reconcile(
    db: &Db,
    project_id: Uuid,
    space_path: &Path,
) -> anyhow::Result<ReconcileReport> {
    let guard = lock_for(project_id);
    let _held = guard.lock().await;

    let mut report = ReconcileReport::default();
    let files = walk(space_path).await?;

    // What the database believes, keyed by rel_path.
    let known: HashMap<String, (Uuid, String, String)> = sqlx::query(
        "SELECT id, rel_path, content_hash, status FROM project_documents WHERE project_id=$1",
    )
    .bind(project_id)
    .fetch_all(&db.pool)
    .await?
    .into_iter()
    .map(|r| {
        (
            r.get::<String, _>("rel_path"),
            (r.get::<Uuid, _>("id"), r.get::<String, _>("content_hash"), r.get::<String, _>("status")),
        )
    })
    .collect();

    let mut seen: Vec<String> = Vec::with_capacity(files.len());
    for abs in files {
        let rel = abs
            .strip_prefix(space_path)
            .unwrap_or(&abs)
            .to_string_lossy()
            .into_owned();
        seen.push(rel.clone());

        let meta = tokio::fs::metadata(&abs).await?;
        if meta.len() > MAX_INDEX_BYTES {
            upsert_status(
                db, project_id, &rel, "", meta.len() as i64, "unsupported",
                Some("too large to index — the assistant can still Read it"),
            )
            .await?;
            report.unsupported += 1;
            continue;
        }

        let bytes = tokio::fs::read(&abs).await?;
        let hash = hex::encode(Sha256::digest(&bytes));
        if let Some((_, known_hash, status)) = known.get(&rel) {
            if *known_hash == hash && status == "indexed" {
                report.unchanged += 1;
                continue;
            }
        }

        // Extraction under spawn_blocking: PDF and office parsing is CPU
        // work, and a parser panic on a malformed file becomes a JoinError
        // here — a failed row with a reason — instead of taking the server
        // down mid-reconcile.
        let extracted = {
            let rel2 = rel.clone();
            let bytes2 = bytes.clone();
            tokio::task::spawn_blocking(move || super::extract::extract(&rel2, &bytes2)).await
        };
        let text = match extracted {
            Ok(Ok(super::extract::Extracted::Text(t))) => t,
            Ok(Ok(super::extract::Extracted::Unsupported(reason))) => {
                upsert_status(
                    db, project_id, &rel, &hash, bytes.len() as i64, "unsupported", Some(reason),
                )
                .await?;
                report.unsupported += 1;
                continue;
            }
            Ok(Err(e)) => {
                upsert_status(
                    db, project_id, &rel, &hash, bytes.len() as i64, "failed",
                    Some(&e.to_string()),
                )
                .await?;
                report.failed += 1;
                continue;
            }
            Err(join) => {
                upsert_status(
                    db, project_id, &rel, &hash, bytes.len() as i64, "failed",
                    Some(&format!("the extractor crashed on this file: {join}")),
                )
                .await?;
                report.failed += 1;
                continue;
            }
        };
        let chunks = super::chunk::chunk(&text);
        if chunks.is_empty() {
            upsert_status(db, project_id, &rel, &hash, bytes.len() as i64, "unsupported", Some("empty file"))
                .await?;
            report.unsupported += 1;
            continue;
        }

        match super::embed::embed_batch(chunks.clone()).await {
            Ok(embeddings) => {
                // One transaction per document: the old chunk set and the new
                // one must never be visible together.
                let mut tx = db.pool.begin().await?;
                let doc_id: Uuid = sqlx::query_scalar(
                    "INSERT INTO project_documents (project_id, rel_path, content_hash, bytes,
                                                  status, error, indexed_at)
                     VALUES ($1, $2, $3, $4, 'indexed', NULL, now())
                     ON CONFLICT (project_id, rel_path) DO UPDATE SET
                        content_hash=EXCLUDED.content_hash, bytes=EXCLUDED.bytes,
                        status='indexed', error=NULL, indexed_at=now()
                     RETURNING id",
                )
                .bind(project_id)
                .bind(&rel)
                .bind(&hash)
                .bind(bytes.len() as i64)
                .fetch_one(&mut *tx)
                .await?;
                sqlx::query("DELETE FROM project_chunks WHERE document_id=$1")
                    .bind(doc_id)
                    .execute(&mut *tx)
                    .await?;
                for (i, (content, emb)) in chunks.iter().zip(&embeddings).enumerate() {
                    sqlx::query(
                        "INSERT INTO project_chunks (document_id, project_id, chunk_index,
                                                   content, embedding, embedding_model)
                         VALUES ($1, $2, $3, $4, $5, $6)",
                    )
                    .bind(doc_id)
                    .bind(project_id)
                    .bind(i as i32)
                    .bind(content)
                    .bind(super::embed::to_bytes(emb))
                    .bind(super::embed::MODEL_TAG)
                    .execute(&mut *tx)
                    .await?;
                }
                tx.commit().await?;
                report.indexed += 1;
            }
            Err(e) => {
                // Kept, with the reason: the rail shows it and the next
                // reconcile retries — the commonest cause is a network blip
                // during the one-time model download.
                upsert_status(
                    db, project_id, &rel, &hash, bytes.len() as i64, "failed",
                    Some(&e.to_string()),
                )
                .await?;
                report.failed += 1;
            }
        }
    }

    // Rows whose file is gone. The cascade takes the chunks.
    let removed = sqlx::query(
        "DELETE FROM project_documents WHERE project_id=$1 AND NOT (rel_path = ANY($2))",
    )
    .bind(project_id)
    .bind(&seen)
    .execute(&db.pool)
    .await?
    .rows_affected();
    report.removed = removed as usize;

    Ok(report)
}

async fn upsert_status(
    db: &Db,
    project_id: Uuid,
    rel: &str,
    hash: &str,
    bytes: i64,
    status: &str,
    error: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO project_documents (project_id, rel_path, content_hash, bytes, status, error)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (project_id, rel_path) DO UPDATE SET
            content_hash=EXCLUDED.content_hash, bytes=EXCLUDED.bytes,
            status=EXCLUDED.status, error=EXCLUDED.error",
    )
    .bind(project_id)
    .bind(rel)
    .bind(hash)
    .bind(bytes)
    .bind(status)
    .bind(error)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// Every regular file under the root, skipping dot-entries and symlinks.
async fn walk(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue, // a folder deleted mid-walk is not an error
        };
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let ft = entry.file_type().await?;
            if ft.is_symlink() {
                continue; // a symlink out of the space is a folder escape
            }
            if ft.is_dir() {
                stack.push(entry.path());
            } else if ft.is_file() {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    Ok(out)
}


