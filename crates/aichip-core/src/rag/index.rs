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

/// Files bigger than this are `unsupported` for indexing. Generous for prose
/// — ~500 chunks — while keeping one stray dump from monopolizing the
/// embedder. The agent's own Read still opens them in-folder.
pub const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;

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

/// Walk the space's folder and bring `space_documents`/`space_chunks` in
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
        "SELECT id, rel_path, content_hash, status FROM space_documents WHERE project_id=$1",
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

        if !looks_like_text(&bytes) {
            upsert_status(
                db, project_id, &rel, &hash, bytes.len() as i64, "unsupported",
                Some("not a text file — the assistant can still Read it in the folder"),
            )
            .await?;
            report.unsupported += 1;
            continue;
        }

        let text = String::from_utf8_lossy(&bytes).into_owned();
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
                    "INSERT INTO space_documents (project_id, rel_path, content_hash, bytes,
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
                sqlx::query("DELETE FROM space_chunks WHERE document_id=$1")
                    .bind(doc_id)
                    .execute(&mut *tx)
                    .await?;
                for (i, (content, emb)) in chunks.iter().zip(&embeddings).enumerate() {
                    sqlx::query(
                        "INSERT INTO space_chunks (document_id, project_id, chunk_index,
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
        "DELETE FROM space_documents WHERE project_id=$1 AND NOT (rel_path = ANY($2))",
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
        "INSERT INTO space_documents (project_id, rel_path, content_hash, bytes, status, error)
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

/// Valid UTF-8 with no NUL and few control characters. A copy of the
/// heuristic `routes/kb.rs` uses (core cannot depend on the server crate) —
/// "valid UTF-8 with no NUL is not enough on its own; an ELF header passes
/// both".
fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    let Ok(s) = std::str::from_utf8(bytes) else {
        return false;
    };
    let control = s
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        .count();
    control * 100 <= s.chars().count().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_detection_accepts_prose_and_rejects_binaries() {
        assert!(looks_like_text(b"# Handbook\n\nDeploys on Tuesday.\n"));
        assert!(looks_like_text("日本語のノート".as_bytes()));
        assert!(!looks_like_text(b"\x7fELF\x02\x01\x01\x00\x00"));
        assert!(!looks_like_text(b"%PDF-1.7\x00binary"));
        assert!(!looks_like_text(&[0xff, 0xfe, 0x00, 0x41]));
    }
}
