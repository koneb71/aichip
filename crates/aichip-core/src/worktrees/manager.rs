//! Git worktree lifecycle for task isolation. Worktrees live under
//! `~/.aichip/worktrees/<project-hash>/<task-id>` — outside the user's repo,
//! so agent runs never dirty the main checkout. All git invocations use
//! explicit arg vectors (never shell strings).

use std::path::{Path, PathBuf};
use tokio::process::Command;
use uuid::Uuid;

pub struct WorktreeManager {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

impl WorktreeManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_root() -> PathBuf {
        dirs_home().join(".aichip").join("worktrees")
    }

    /// True when `path` is inside this manager's root — the gate FullAuto
    /// runs must pass.
    pub fn manages(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    pub async fn create(
        &self,
        repo: &Path,
        base_branch: &str,
        task_id: Uuid,
        slug: &str,
    ) -> anyhow::Result<Worktree> {
        let project_hash = short_hash(&repo.to_string_lossy());
        let path = self.root.join(project_hash).join(task_id.to_string());
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        let branch = format!("aichip/{slug}-{}", &task_id.to_string()[..8]);
        git(
            repo,
            &[
                "worktree",
                "add",
                path.to_str().unwrap(),
                "-b",
                &branch,
                base_branch,
            ],
        )
        .await?;
        Ok(Worktree { path, branch })
    }

    /// Unified diff of everything the agent changed (committed + uncommitted)
    /// relative to the base branch.
    pub async fn diff(&self, worktree: &Path, base_branch: &str) -> anyhow::Result<String> {
        git(worktree, &["add", "-N", "."]).await?; // make untracked files diffable
        git(worktree, &["diff", base_branch]).await
    }

    /// Squash-merge the worktree branch back onto the base branch in the
    /// main repo. Returns Err with the git output on conflicts.
    pub async fn squash_merge(
        &self,
        repo: &Path,
        worktree: &Worktree,
        base_branch: &str,
        message: &str,
    ) -> anyhow::Result<()> {
        // Commit any uncommitted agent work in the worktree first.
        git(&worktree.path, &["add", "-A"]).await?;
        let status = git(&worktree.path, &["status", "--porcelain"]).await?;
        if !status.trim().is_empty() {
            git(&worktree.path, &["commit", "-m", message]).await?;
        }
        git(repo, &["checkout", base_branch]).await?;
        git(repo, &["merge", "--squash", &worktree.branch]).await?;
        git(repo, &["commit", "-m", message]).await?;
        Ok(())
    }

    pub async fn remove(&self, repo: &Path, worktree: &Worktree) -> anyhow::Result<()> {
        git(
            repo,
            &[
                "worktree",
                "remove",
                "--force",
                worktree.path.to_str().unwrap(),
            ],
        )
        .await?;
        let _ = git(repo, &["branch", "-D", &worktree.branch]).await;
        Ok(())
    }
}

async fn git(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git").current_dir(cwd).args(args).output().await?;
    if !out.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn short_hash(s: &str) -> String {
    // Stable, dependency-free path hash (FNV-1a).
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn init_repo(dir: &Path) {
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@aichip.local"],
            vec!["config", "user.name", "aichip-test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            git(dir, &args).await.unwrap();
        }
    }

    #[tokio::test]
    async fn create_diff_merge_roundtrip() {
        let repo_dir = tempfile::tempdir().unwrap();
        let wt_root = tempfile::tempdir().unwrap();
        init_repo(repo_dir.path()).await;

        let mgr = WorktreeManager::new(wt_root.path());
        let task_id = Uuid::new_v4();
        let wt = mgr
            .create(repo_dir.path(), "main", task_id, "add-hello")
            .await
            .unwrap();
        assert!(mgr.manages(&wt.path));

        tokio::fs::write(wt.path.join("hello.txt"), "hi\n").await.unwrap();
        let diff = mgr.diff(&wt.path, "main").await.unwrap();
        assert!(diff.contains("hello.txt"));

        mgr.squash_merge(repo_dir.path(), &wt, "main", "add hello")
            .await
            .unwrap();
        let log = git(repo_dir.path(), &["log", "--oneline"]).await.unwrap();
        assert!(log.contains("add hello"));
        assert!(repo_dir.path().join("hello.txt").exists());

        mgr.remove(repo_dir.path(), &wt).await.unwrap();
        assert!(!wt.path.exists());
    }
}
