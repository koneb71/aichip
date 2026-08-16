//! Worktrees nothing will ever look at again.
//!
//! The ordinary lifecycle is fine: a card's worktree is reclaimed when the card
//! merges, is deleted, or is retried from a clean checkout. What this module is
//! for is the directories that fall out of that lifecycle entirely, where no
//! amount of clicking in the dashboard can reach them:
//!
//! * **Bake-off variants.** Their path lives in `runs.worktree_path`, and
//!   `runs.task_id` is `ON DELETE CASCADE` — delete the card and the rows
//!   holding the paths vanish while the directories stay.
//! * **Workflow fan-out.** `orchestrator` creates one per parallel step keyed
//!   by a fresh uuid that is *never persisted anywhere*. Orphaned at creation.
//! * **Uninstalled apps.** The project row and the folder both go, so
//!   `git worktree remove` can never succeed against that repository again —
//!   the repository is not there to run it.
//!
//! Modelled on `previews::reconcile` and `attachments::sweep_abandoned`, which
//! exist for the same reason: Postgres can drop a row but not a directory.
//!
//! Conservative in the same way the on-demand reclaim is. A directory is only
//! removed when it is provably finished with — landed and clean — or when the
//! repository it belongs to is gone, in which case there is nothing left to
//! merge it into.

use crate::db::Db;
use crate::worktrees::manager::{self, Worktree, WorktreeManager};
use sqlx::Row;
use std::collections::HashSet;
use std::path::Path;

/// What one sweep freed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub worktrees: usize,
    pub bytes: u64,
    /// Whole project directories whose repository no longer exists.
    pub dead_projects: usize,
}

/// Release worktrees no row claims and no repository wants.
///
/// Runs at boot rather than on a timer: these accumulate through events —
/// a card deleted, an app uninstalled — and a sweep that catches them at the
/// next start is soon enough for disk. Never fatal; a failure here costs space,
/// not correctness.
pub async fn reconcile(db: &Db, worktrees: &WorktreeManager) -> anyhow::Result<Swept> {
    let projects = sqlx::query("SELECT id, path, default_branch FROM projects WHERE vcs = 'git'")
        .fetch_all(&db.pool)
        .await?;

    let mut swept = Swept::default();

    // Every path still spoken for. Both columns: a card's own worktree and a
    // bake-off variant's, which live in different tables and are equally alive.
    let claimed: HashSet<String> = sqlx::query_scalar::<_, String>(
        "SELECT worktree_path FROM tasks WHERE worktree_path IS NOT NULL
         UNION
         SELECT worktree_path FROM runs WHERE worktree_path IS NOT NULL",
    )
    .fetch_all(&db.pool)
    .await?
    .into_iter()
    .collect();

    for p in &projects {
        let repo = p.get::<String, _>("path");
        let repo = Path::new(&repo);
        if !repo.is_dir() {
            continue;
        }
        let base: String = p.get("default_branch");
        let held = match manager::inventory(repo, &base).await {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!(repo = %repo.display(), error = %e, "could not inventory worktrees");
                continue;
            }
        };
        for h in held {
            if claimed.contains(&h.path.to_string_lossy().to_string()) || !h.reclaimable() {
                continue;
            }
            let wt = Worktree {
                path: h.path.clone(),
                branch: h.branch.clone(),
            };
            match worktrees.remove(repo, &wt).await {
                Ok(()) => {
                    tracing::info!(branch = %h.branch, bytes = h.bytes, "swept an unclaimed worktree");
                    swept.worktrees += 1;
                    swept.bytes += h.bytes;
                }
                Err(e) => {
                    tracing::warn!(branch = %h.branch, error = %e, "could not sweep worktree")
                }
            }
        }
    }

    swept.dead_projects = sweep_dead_projects(worktrees, &projects).await;
    Ok(swept)
}

/// Directories under the worktree root belonging to a repository that is gone.
///
/// The uninstalled-app case, and the only one that cannot be fixed by git:
/// with no repository there is no `git worktree remove` to run, so this is a
/// plain directory removal. Bounded by construction — it only ever looks inside
/// aichip's own root, and only at names that are the project-path hash, so a
/// name that matches no live project cannot be anything else.
///
/// Skipped entirely when there are no projects. A database that failed to load
/// is not evidence that every worktree on disk is garbage.
async fn sweep_dead_projects(
    worktrees: &WorktreeManager,
    projects: &[sqlx::postgres::PgRow],
) -> usize {
    if projects.is_empty() {
        return 0;
    }
    let live: HashSet<String> = projects
        .iter()
        .map(|p| WorktreeManager::project_dir_name(&p.get::<String, _>("path")))
        .collect();

    let root = worktrees.root();
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return 0;
    };
    let mut removed = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if live.contains(&name) {
            continue;
        }
        if !entry.metadata().await.map(|m| m.is_dir()).unwrap_or(false) {
            continue;
        }
        match tokio::fs::remove_dir_all(entry.path()).await {
            Ok(()) => {
                tracing::info!(dir = %name, "swept worktrees of a project that no longer exists");
                removed += 1;
            }
            Err(e) => tracing::warn!(dir = %name, error = %e, "could not sweep dead project dir"),
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_projects_directory_name_is_stable() {
        // The sweep decides what to delete by comparing these, so a hash that
        // changed between runs would be a hash that deletes live worktrees.
        let a = WorktreeManager::project_dir_name("/Users/x/code/thing");
        let b = WorktreeManager::project_dir_name("/Users/x/code/thing");
        assert_eq!(a, b);
        assert_ne!(a, WorktreeManager::project_dir_name("/Users/x/code/other"));
        assert!(!a.is_empty());
    }
}
