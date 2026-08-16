//! Files keyed by something that no longer exists.
//!
//! aichip writes one small file per run and per preview, in three places, and
//! until this module none of the three was ever cleaned:
//!
//! * `~/.aichip/mcp/<run_id>.json` — the generated MCP config a run is spawned
//!   with. One per run, deliberately not shared
//!   (`claude/mcp.rs`: *"One file per run rather than a shared one"*).
//! * `~/.aichip/prompts/<run_id>.md` — OpenCode's instructions file, same shape.
//! * `~/.aichip/previews/aichip-preview-<short>.log` and `.out` — a preview's
//!   build and runtime output. Its own doc says *"build logs are megabytes"*,
//!   and nothing removed them: not `stop`, not `sweep_idle`, not `reconcile`,
//!   not the cascade when the row went.
//!
//! Individually small, unbounded in count, and all keyed by an id that the
//! database can be asked about — which is what makes a sweep possible at all.
//! Same reasoning as `attachments::sweep_abandoned` and `worktrees::sweep`:
//! Postgres can drop a row but not a file.
//!
//! Conservative in the same way: a file is only removed when its owner is
//! *known to be finished*. Anything this cannot identify — a name that is not
//! an id, an extension it does not write — is left alone, because a directory
//! under `~/.aichip` is still the user's disk.

use crate::db::Db;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub files: usize,
    pub bytes: u64,
}

fn home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".aichip")
}

/// Remove the per-run and per-preview files whose owner is finished with.
///
/// Boot rather than a timer: these accumulate one per run, so a sweep at the
/// next start is soon enough for a few kilobytes each, and it keeps the number
/// of background loops down. Never fatal — a failure costs disk, not
/// correctness.
pub async fn sweep(db: &Db) -> anyhow::Result<Swept> {
    // A run still owing an outcome is still using its config file. Everything
    // else has been spawned and finished; the file is a spent argument.
    let live_runs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM runs WHERE status NOT IN ('completed','failed','canceled')",
    )
    .fetch_all(&db.pool)
    .await?;
    let live_runs: HashSet<String> = live_runs.iter().map(Uuid::to_string).collect();

    // A preview that can still be woken can still be asked for its logs — and
    // the logs are the only account of a *failed* build, so a failed row keeps
    // them too. `stopped` does not: nothing offers to show them again.
    let wakeable: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM previews WHERE status IN ('building','running','idle','failed')",
    )
    .fetch_all(&db.pool)
    .await?;
    let wakeable: HashSet<String> = wakeable
        .iter()
        .map(crate::previews::recipe::container_name)
        .collect();

    let mut swept = Swept::default();
    for (dir, exts, keep) in [
        (home().join("mcp"), &["json"][..], &live_runs),
        (home().join("prompts"), &["md"][..], &live_runs),
        (home().join("previews"), &["log", "out"][..], &wakeable),
    ] {
        swept = add(swept, sweep_dir(&dir, exts, keep).await);
    }
    Ok(swept)
}

fn add(a: Swept, b: Swept) -> Swept {
    Swept {
        files: a.files + b.files,
        bytes: a.bytes + b.bytes,
    }
}

/// One directory: remove files whose stem is not in `keep`.
///
/// The extension check is what keeps this to files aichip wrote. A directory
/// under `~/.aichip` is still the user's disk, and something in there this
/// module does not recognise is not evidence that it is rubbish.
async fn sweep_dir(dir: &Path, exts: &[&str], keep: &HashSet<String>) -> Swept {
    let mut swept = Swept::default();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return swept;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !exts.contains(&ext) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if keep.contains(stem) {
            continue;
        }
        let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                swept.files += 1;
                swept.bytes += size;
            }
            Err(e) => tracing::warn!(path = %path.display(), error = %e, "could not sweep"),
        }
    }
    swept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn removes_only_what_it_recognises_and_nobody_owns() {
        let dir = tempfile::tempdir().unwrap();
        let live = Uuid::new_v4();
        let dead = Uuid::new_v4();

        for (name, body) in [
            (format!("{live}.json"), "still running"),
            (format!("{dead}.json"), "finished"),
            // Not an id at all, and not an extension this writes. Both are
            // somebody else's file until proven otherwise.
            ("notes.json".to_string(), "a person's"),
            (format!("{dead}.txt"), "not ours"),
        ] {
            tokio::fs::write(dir.path().join(name), body).await.unwrap();
        }

        let keep: HashSet<String> = [live.to_string()].into_iter().collect();
        let swept = sweep_dir(dir.path(), &["json"], &keep).await;

        assert_eq!(swept.files, 2, "the dead id and the unrecognised name");
        assert!(
            dir.path().join(format!("{live}.json")).exists(),
            "a live run keeps its config"
        );
        assert!(!dir.path().join(format!("{dead}.json")).exists());
        assert!(
            dir.path().join(format!("{dead}.txt")).exists(),
            "an extension aichip does not write is not aichip's to remove"
        );
        assert!(swept.bytes > 0, "the freed bytes are worth reporting");
    }

    #[tokio::test]
    async fn a_missing_directory_is_not_an_error() {
        // A fresh install has none of these until the first run.
        let swept = sweep_dir(
            Path::new("/nonexistent/aichip/mcp"),
            &["json"],
            &HashSet::new(),
        )
        .await;
        assert_eq!(swept, Swept::default());
    }
}
