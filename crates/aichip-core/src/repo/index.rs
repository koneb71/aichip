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
    let stored: Option<String> =
        sqlx::query_scalar("SELECT head_sha FROM project_index WHERE project_id = $1")
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

    // A file whose bytes have not moved still needs re-reading when the thing
    // that reads it has changed. Asked once, before the loop, so it costs one
    // query rather than one per file.
    let reparse_all: bool = sqlx::query_scalar::<_, i32>(
        "SELECT parse_version FROM project_index WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten()
    .map(|v| v != super::symbols::PARSE_VERSION)
    .unwrap_or(true);

    let mut changed: Vec<Changed> = vec![];
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
            // 'pending' and 'failed' retry; 'indexed' and 'unsupported' are
            // both settled answers about a file whose bytes have not moved.
            if !reparse_all
                && known_hash == &hash
                && matches!(status.as_str(), "indexed" | "unsupported")
            {
                report.unchanged += 1;
                continue;
            }
        }
        // Binary or otherwise unreadable as text: skipped, not failed. The
        // agent can still open it; it just is not searchable by meaning.
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        // Parsed here, in the read that already happened. A separate symbol
        // pass would re-read and re-hash every file in the project to learn
        // what this loop is holding in a local variable.
        let lang = super::symbols::Lang::of(rel);
        let parsed = lang
            .map(|l| super::symbols::parse(l, &text))
            .unwrap_or_default();
        // A parser bump changes what we know about a file, not what is in it.
        // Re-embedding 176 unchanged files to learn their function names would
        // cost minutes and produce byte-identical vectors.
        let settled = known
            .get(rel)
            .is_some_and(|(_, h, s)| h == &hash && matches!(s.as_str(), "indexed" | "unsupported"));
        changed.push(Changed {
            rel: rel.clone(),
            hash,
            bytes: meta.len(),
            lang: lang.map(|l| l.tag()),
            chunks: super::chunk::chunk_code(&text),
            parsed,
            needs_vectors: !settled,
        });
    }

    // Everything up to here is the fast half, and it lands before the slow
    // half starts: definitions, dependencies, the graph and the ranking are
    // all in the database while the embedder has not been asked for anything.
    // A project with no network still gets its whole map.
    for file in &changed {
        if let Err(e) = write_structure(db, project_id, file).await {
            tracing::warn!(%project_id, rel = %file.rel, error = %e, "structure write failed");
            report.failed += 1;
        }
    }
    // Before the graph, not after: an edge into a file that is no longer on
    // disk would be inserted only to cascade away a moment later.
    report.removed = prune(db, project_id, &paths).await;
    let moved = !changed.is_empty() || report.removed > 0;
    let graph = rebuild_graph(db, project_id).await.unwrap_or_default();

    // Structure is done. Say so before the slow half starts, so the UI can
    // stop claiming it is still reading.
    let _ = sqlx::query(
        "INSERT INTO project_index (project_id, phase, head_sha, files_total, files_parsed,
                                    structure_version, indexed_at, error, note,
                                    edges_total, imports_total, imports_resolved, symbols_total,
                                    parse_version, updated_at)
         VALUES ($1, $2, $3, $4, $5, 1, now(), NULL, $6, $7, $8, $9, $10, $12, now())
         ON CONFLICT (project_id) DO UPDATE SET
             phase = EXCLUDED.phase,
             head_sha = EXCLUDED.head_sha,
             files_total = EXCLUDED.files_total,
             files_parsed = EXCLUDED.files_parsed,
             structure_version = project_index.structure_version
                 + CASE WHEN $11 THEN 1 ELSE 0 END,
             indexed_at = now(),
             error = NULL,
             note = EXCLUDED.note,
             edges_total = EXCLUDED.edges_total,
             imports_total = EXCLUDED.imports_total,
             imports_resolved = EXCLUDED.imports_resolved,
             symbols_total = EXCLUDED.symbols_total,
             parse_version = EXCLUDED.parse_version,
             updated_at = now()",
    )
    .bind(project_id)
    .bind(if changed.is_empty() { "ready" } else { "embedding" })
    .bind(head_sha(&root).await)
    .bind(paths.len() as i32)
    .bind(graph.parsed_files)
    .bind(paths.is_empty().then_some(
        "Nothing here in a language this understands yet — the map fills in when there is code to read.",
    ))
    .bind(graph.edges)
    .bind(graph.imports_total)
    .bind(graph.imports_resolved)
    .bind(graph.symbols)
    .bind(moved)
    .bind(super::symbols::PARSE_VERSION)
    .execute(&db.pool)
    .await;

    // The embedder is *not* pre-checked, and that is load-bearing: the model
    // downloads lazily on the first `embed_batch`, so gating on `status()`
    // being Ready meant nothing ever asked, the download never started, and
    // the vectors phase waited forever on a condition only it could satisfy.
    // Ask, and let the failure be the answer.
    for Changed {
        rel,
        hash,
        bytes,
        chunks,
        needs_vectors,
        ..
    } in changed
    {
        if !needs_vectors {
            report.unchanged += 1; // re-read, not re-embedded
            continue;
        }
        if chunks.is_empty() {
            continue; // structured, nothing to embed — a state, not a failure
        }
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

    settle(db, project_id, root, report).await
}

/// One changed file, everything the pass learned about it in one read.
struct Changed {
    rel: String,
    hash: String,
    bytes: u64,
    /// `None` for a file no grammar here can read — a state, not a failure.
    lang: Option<&'static str>,
    chunks: Vec<super::chunk::CodeChunk>,
    parsed: super::symbols::Parsed,
    /// False when only the parser moved: the bytes are the same, so the
    /// vectors already in the database are the vectors this text would
    /// produce, and the file's `status` must not be knocked back either.
    needs_vectors: bool,
}

/// What one file is and what it says, replaced wholesale in one transaction.
///
/// Wholesale rather than patched, and for the same reason the chunk set is: a
/// half-updated symbol list names a function at a line it has moved away from,
/// and being sent to the wrong line is worse than being sent nowhere.
async fn write_structure(db: &Db, project_id: Uuid, file: &Changed) -> anyhow::Result<()> {
    let mut tx = db.pool.begin().await?;
    // 'pending' means seen and not yet embedded; 'unsupported' means there was
    // nothing to embed, which is settled rather than pending and so does not
    // make the next pass read the file again.
    // NULL leaves the status where it was — the re-parse-only case, where
    // knocking an indexed file back to 'pending' would take it out of search
    // for no reason at all.
    let status: Option<&str> = file.needs_vectors.then(|| {
        if file.chunks.is_empty() {
            "unsupported"
        } else {
            "pending"
        }
    });
    let doc_id: Uuid = sqlx::query_scalar(
        "INSERT INTO project_documents (project_id, rel_path, content_hash, bytes, lang, status, error)
         VALUES ($1,$2,$3,$4,$5,COALESCE($6,'pending'),NULL)
         ON CONFLICT (project_id, rel_path) DO UPDATE SET
             content_hash = EXCLUDED.content_hash, bytes = EXCLUDED.bytes,
             lang = EXCLUDED.lang,
             status = COALESCE($6, project_documents.status),
             error = CASE WHEN $6 IS NULL THEN project_documents.error ELSE NULL END
         RETURNING id",
    )
    .bind(project_id)
    .bind(&file.rel)
    .bind(&file.hash)
    .bind(file.bytes as i64)
    .bind(file.lang)
    .bind(status)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM project_symbols WHERE document_id = $1")
        .bind(doc_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM project_imports WHERE document_id = $1")
        .bind(doc_id)
        .execute(&mut *tx)
        .await?;

    for s in &file.parsed.symbols {
        sqlx::query(
            "INSERT INTO project_symbols (project_id, document_id, name, kind, line, signature)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(project_id)
        .bind(doc_id)
        .bind(&s.name)
        .bind(s.kind)
        .bind(s.line)
        .bind(s.signature.as_deref())
        .execute(&mut *tx)
        .await?;
    }
    for i in &file.parsed.imports {
        sqlx::query(
            "INSERT INTO project_imports (project_id, document_id, specifier, line)
             VALUES ($1,$2,$3,$4)",
        )
        .bind(project_id)
        .bind(doc_id)
        .bind(&i.specifier)
        .bind(i.line)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// What the graph pass found, for the index row to report.
#[derive(Default)]
struct GraphReport {
    parsed_files: i32,
    symbols: i32,
    edges: i32,
    imports_total: i32,
    imports_resolved: i32,
}

/// Resolve every stored specifier and rebuild the edge table and the ranking.
///
/// Whole, every pass, rather than incrementally. Resolution is a function of
/// the entire file set — adding one file can make a previously dangling
/// specifier point somewhere, and removing one can orphan an edge that was
/// fine yesterday — so an incremental update would be wrong in exactly the
/// cases that matter. The whole thing is a few thousand string comparisons.
async fn rebuild_graph(db: &Db, project_id: Uuid) -> anyhow::Result<GraphReport> {
    let files: Vec<(Uuid, String, Option<String>)> =
        sqlx::query("SELECT id, rel_path, lang FROM project_documents WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(&db.pool)
            .await?
            .into_iter()
            .map(|r| {
                (
                    r.get::<Uuid, _>("id"),
                    r.get::<String, _>("rel_path"),
                    r.get::<Option<String>, _>("lang"),
                )
            })
            .collect();

    let paths: Vec<String> = files.iter().map(|(_, p, _)| p.clone()).collect();
    let index_of: HashMap<&str, usize> = files
        .iter()
        .enumerate()
        .map(|(i, (_, p, _))| (p.as_str(), i))
        .collect();

    // What each name is defined in, so a Rust path naming a type rather than a
    // module still finds its file.
    let mut defines: HashMap<String, Vec<String>> = HashMap::new();
    let symbol_rows = sqlx::query(
        "SELECT s.name, d.rel_path
         FROM project_symbols s JOIN project_documents d ON d.id = s.document_id
         WHERE s.project_id = $1",
    )
    .bind(project_id)
    .fetch_all(&db.pool)
    .await?;
    for r in &symbol_rows {
        defines
            .entry(r.get::<String, _>("name"))
            .or_default()
            .push(r.get::<String, _>("rel_path"));
    }
    let set = super::imports::PathSet::new(&paths).with_symbols(defines);

    let import_rows = sqlx::query(
        "SELECT d.rel_path AS from_path, i.specifier
         FROM project_imports i JOIN project_documents d ON d.id = i.document_id
         WHERE i.project_id = $1",
    )
    .bind(project_id)
    .fetch_all(&db.pool)
    .await?;

    let mut weights: HashMap<(usize, usize), i32> = HashMap::new();
    let mut resolved = 0i32;
    for r in &import_rows {
        let from_path = r.get::<String, _>("from_path");
        let spec = r.get::<String, _>("specifier");
        let Some(to_path) = super::imports::resolve(&spec, &from_path, &set) else {
            continue; // names a package, or names nothing — dropped, never guessed
        };
        resolved += 1;
        let (Some(&a), Some(&b)) = (
            index_of.get(from_path.as_str()),
            index_of.get(to_path.as_str()),
        ) else {
            continue;
        };
        if a != b {
            *weights.entry((a, b)).or_insert(0) += 1;
        }
    }

    let pairs: Vec<(usize, usize)> = weights.keys().copied().collect();
    let ranks = super::rank::pagerank(files.len(), &pairs);

    let (mut from_ids, mut to_ids, mut ws) = (vec![], vec![], vec![]);
    for (&(a, b), &w) in &weights {
        from_ids.push(files[a].0);
        to_ids.push(files[b].0);
        ws.push(w);
    }

    // One transaction: a canvas reading between the delete and the insert
    // would draw a project with no dependencies at all.
    let mut tx = db.pool.begin().await?;
    sqlx::query("DELETE FROM project_edges WHERE project_id = $1")
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO project_edges (project_id, from_document, to_document, weight)
         SELECT $1, f, t, w FROM unnest($2::uuid[], $3::uuid[], $4::int[]) AS x(f, t, w)",
    )
    .bind(project_id)
    .bind(&from_ids)
    .bind(&to_ids)
    .bind(&ws)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE project_documents SET rank = r.v
         FROM (SELECT unnest($1::uuid[]) AS id, unnest($2::real[]) AS v) r
         WHERE project_documents.id = r.id",
    )
    .bind(files.iter().map(|(id, _, _)| *id).collect::<Vec<_>>())
    .bind(&ranks)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(GraphReport {
        parsed_files: files.iter().filter(|(_, _, l)| l.is_some()).count() as i32,
        symbols: symbol_rows.len() as i32,
        edges: weights.len() as i32,
        imports_total: import_rows.len() as i32,
        imports_resolved: resolved,
    })
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
        anyhow::bail!(
            "the embedder returned {} vectors for {} chunks",
            vectors.len(),
            chunks.len()
        );
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

/// Drop what is no longer on disk.
///
/// A file deleted from the repository must stop being findable — a map that
/// names a path which no longer exists sends an agent to read a ghost. Its
/// chunks, symbols, imports and edges go with it on the cascade, which is why
/// they all hang off `project_documents` rather than off the project.
async fn prune(db: &Db, project_id: Uuid, paths: &[String]) -> usize {
    sqlx::query("DELETE FROM project_documents WHERE project_id = $1 AND NOT (rel_path = ANY($2))")
        .bind(project_id)
        .bind(paths)
        .execute(&db.pool)
        .await
        .map(|r| r.rows_affected() as usize)
        .unwrap_or(0)
}

/// Settle the phase and the counts.
async fn settle(
    db: &Db,
    project_id: Uuid,
    root: PathBuf,
    report: IndexReport,
) -> anyhow::Result<IndexReport> {
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
    // 'ready' either way, and deliberately: vectors that could not be built
    // are reported on the report and in the embedder's own status, not by
    // parking the index in a phase that says the map is unfinished. The map
    // *is* finished — the structure, the symbols and the graph all landed.
    .bind("ready")
    .bind(embedded as i32)
    .bind(head_sha(&root).await)
    .execute(&db.pool)
    .await;
    Ok(report)
}
