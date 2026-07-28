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

        // A repository added before we checked for commits — or one the user
        // init'd themselves — has an unborn HEAD and nothing to branch from.
        // Writing the root commit is the same thing we do when we create a
        // repo, and the only alternative is failing every run here.
        if !has_commits(repo).await {
            tracing::warn!(
                repo = %repo.display(),
                "repository has no commits; writing an initial commit so runs can be isolated"
            );
            commit_everything(repo).await?;
        }
        let base = resolve_base(repo, base_branch).await?;
        git(
            repo,
            &[
                "worktree",
                "add",
                path.to_str().unwrap(),
                "-b",
                &branch,
                &base,
            ],
        )
        .await?;
        Ok(Worktree { path, branch })
    }

    /// Unified diff of everything the agent changed (committed + uncommitted)
    /// relative to the base branch.
    pub async fn diff(&self, worktree: &Path, base_branch: &str) -> anyhow::Result<String> {
        git(worktree, &["add", "-N", "."]).await?; // make untracked files diffable
        let base = resolve_base(worktree, base_branch).await?;
        git(worktree, &["diff", &base]).await
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
        let base = resolve_base(repo, base_branch).await?;
        git(repo, &["checkout", &base]).await?;
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

/// What a worktree should branch from.
///
/// A project row outlives the branch it names — the branch gets renamed, or
/// the row recorded a default before the repo actually had one — and `git
/// worktree add <path> -b <new> <base>` fails outright on a base it can't
/// resolve. Falling back to whatever the repo is actually on beats failing
/// the run over a stale row.
async fn resolve_base(repo: &Path, preferred: &str) -> anyhow::Result<String> {
    if branch_exists(repo, preferred).await {
        return Ok(preferred.to_string());
    }
    if let Some(actual) = current_branch(repo).await {
        if branch_exists(repo, &actual).await {
            tracing::warn!(
                repo = %repo.display(),
                %preferred, %actual,
                "recorded base branch is missing; using the repository's current branch"
            );
            return Ok(actual);
        }
    }
    if has_commits(repo).await {
        return Ok("HEAD".to_string());
    }
    anyhow::bail!(
        "this repository has no commits yet, so there is nothing to branch from — \
         make one commit in it and start the run again"
    )
}

async fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(repo, &["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .await
        .is_ok()
}

/// The checked-out branch, which resolves even before the first commit.
/// `None` when HEAD is detached.
async fn current_branch(path: &Path) -> Option<String> {
    git(path, &["symbolic-ref", "--short", "HEAD"])
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn has_commits(path: &Path) -> bool {
    git(path, &["rev-parse", "--verify", "HEAD"]).await.is_ok()
}

/// How a project directory is (or isn't) version controlled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vcs {
    /// A usable repository — runs get an isolated worktree, diff, and merge.
    Git,
    /// No repository, with the reason. Runs happen in the directory itself.
    None(String),
}

/// Make `path` usable as a project, initializing a repository if it needs one.
///
/// The worktree is what keeps an agent out of the user's files and what makes
/// review possible, so it is worth creating a repo to get one. When that
/// genuinely can't happen we say why rather than refusing the project.
pub async fn ensure_repo(path: &Path, default_branch: &str) -> Vcs {
    ensure_repo_state(path, default_branch).await.vcs
}

/// How a project ended up version controlled, plus the branch its runs
/// should actually branch from — which is the repository's own branch, not
/// whatever default the caller guessed.
pub struct RepoState {
    pub vcs: Vcs,
    pub branch: String,
}

pub async fn ensure_repo_state(path: &Path, default_branch: &str) -> RepoState {
    if path.join(".git").exists() {
        let branch = current_branch(path)
            .await
            .unwrap_or_else(|| default_branch.to_string());

        // An existing repository with no commits has an unborn HEAD, which
        // nothing can branch from. Being a repo isn't enough to be usable.
        if !has_commits(path).await {
            if let Err(e) = commit_everything(path).await {
                return RepoState {
                    vcs: Vcs::None(format!(
                        "the repository here has no commits and one could not be \
                         created ({e}) — commit something and re-add the folder"
                    )),
                    branch,
                };
            }
        }
        return RepoState {
            vcs: Vcs::Git,
            branch,
        };
    }
    ensure_repo_uninitialized(path, default_branch).await
}

async fn ensure_repo_uninitialized(path: &Path, default_branch: &str) -> RepoState {
    let branch = default_branch.to_string();

    // A folder inside another repository is already tracked by it. Running
    // `git init` here would nest a second repo inside the first, which
    // confuses every later git call for no benefit.
    if let Ok(top) = git(path, &["rev-parse", "--show-toplevel"]).await {
        let top = top.trim();
        if !top.is_empty() {
            return RepoState {
                vcs: Vcs::None(format!(
                    "inside the repository at {top} — add that folder instead to get \
                     isolated worktrees and reviewable diffs"
                )),
                branch,
            };
        }
    }

    match init_repo(path, default_branch).await {
        Ok(()) => RepoState {
            vcs: Vcs::Git,
            branch,
        },
        Err(e) => RepoState {
            vcs: Vcs::None(format!("could not initialize a repository here: {e}")),
            branch,
        },
    }
}

/// `git init` plus a real first commit.
///
/// The commit is not optional and must include existing files: `git worktree
/// add <path> -b <branch> <base>` resolves `<base>` to a commit, so an unborn
/// HEAD fails outright and an *empty* first commit would hand the agent a
/// worktree with none of the user's files in it.
async fn init_repo(path: &Path, default_branch: &str) -> anyhow::Result<()> {
    git(path, &["init", "-b", default_branch]).await?;
    commit_everything(path).await
}

/// Stage everything and write a root commit, giving an unborn HEAD something
/// to branch from. Existing files are included deliberately: an empty first
/// commit would hand the agent a worktree with none of the user's work in it.
async fn commit_everything(path: &Path) -> anyhow::Result<()> {
    git(path, &["add", "-A"]).await?;
    // `-c` rather than `config`: identity is needed for this one commit and
    // writing it into the user's repo config would be presumptuous.
    git(
        path,
        &[
            "-c",
            "user.name=aichip",
            "-c",
            "user.email=aichip@localhost",
            "commit",
            "--allow-empty",
            "-m",
            "Initial commit",
        ],
    )
    .await?;
    Ok(())
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
    async fn ensure_repo_makes_a_fresh_folder_immediately_worktree_able() {
        // The regression this guards: `git init` alone leaves an unborn HEAD,
        // so `git worktree add … main` fails and the project's first task run
        // dies before the engine ever starts.
        let dir = tempfile::tempdir().unwrap();
        let wt_root = tempfile::tempdir().unwrap();
        assert_eq!(ensure_repo(dir.path(), "main").await, Vcs::Git);

        let mgr = WorktreeManager::new(wt_root.path());
        mgr.create(dir.path(), "main", Uuid::new_v4(), "first-task")
            .await
            .expect("a freshly initialized project must support a worktree");
    }

    #[tokio::test]
    async fn existing_files_survive_into_the_worktree() {
        // An *empty* initial commit would satisfy git but hand the agent a
        // worktree with none of the user's files in it.
        let dir = tempfile::tempdir().unwrap();
        let wt_root = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("notes.md"), "existing work\n")
            .await
            .unwrap();
        assert_eq!(ensure_repo(dir.path(), "main").await, Vcs::Git);

        let mgr = WorktreeManager::new(wt_root.path());
        let wt = mgr
            .create(dir.path(), "main", Uuid::new_v4(), "t")
            .await
            .unwrap();
        let carried = tokio::fs::read_to_string(wt.path.join("notes.md")).await.unwrap();
        assert_eq!(carried, "existing work\n");
    }

    #[tokio::test]
    async fn ensure_repo_leaves_an_existing_repo_alone() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path()).await;
        git(dir.path(), &["checkout", "-b", "trunk"]).await.unwrap();
        assert_eq!(ensure_repo(dir.path(), "main").await, Vcs::Git);
        // Still on the branch the user had; init would have reset this.
        let branch = git(dir.path(), &["rev-parse", "--abbrev-ref", "HEAD"]).await.unwrap();
        assert_eq!(branch.trim(), "trunk");
    }

    #[tokio::test]
    async fn a_folder_inside_a_repo_is_not_nested_with_a_second_one() {
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path()).await;
        let inner = repo.path().join("packages/web");
        tokio::fs::create_dir_all(&inner).await.unwrap();

        match ensure_repo(&inner, "main").await {
            Vcs::None(reason) => assert!(
                reason.contains("inside the repository"),
                "reason should point at the parent repo, got: {reason}"
            ),
            Vcs::Git => panic!("must not create a repo nested inside another"),
        }
        assert!(!inner.join(".git").exists());
    }

    /// The reported failure: `git worktree add … main` dies with
    /// "invalid reference: main" because the repo has no commits.
    #[tokio::test]
    async fn an_existing_repo_with_no_commits_is_made_usable() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-b", "main"]).await.unwrap();
        tokio::fs::write(repo.path().join("app.py"), "print('hi')\n")
            .await
            .unwrap();
        assert!(!has_commits(repo.path()).await, "precondition: unborn HEAD");

        let state = ensure_repo_state(repo.path(), "main").await;
        assert_eq!(state.vcs, Vcs::Git);
        assert_eq!(state.branch, "main");
        assert!(has_commits(repo.path()).await);

        // The user's existing file must be in that first commit, or the agent
        // gets a worktree with none of their work in it.
        let tracked = git(repo.path(), &["ls-files"]).await.unwrap();
        assert!(tracked.contains("app.py"));

        let wt_root = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(wt_root.path());
        let wt = mgr
            .create(repo.path(), "main", Uuid::new_v4(), "software-team")
            .await
            .expect("worktree creation should now succeed");
        assert!(wt.path.join("app.py").exists());
    }

    #[tokio::test]
    async fn reports_the_actual_branch_rather_than_the_assumed_one() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-b", "master"]).await.unwrap();
        let state = ensure_repo_state(repo.path(), "main").await;
        assert_eq!(state.branch, "master");
    }

    #[tokio::test]
    async fn a_stale_base_branch_falls_back_instead_of_failing_the_run() {
        let repo = tempfile::tempdir().unwrap();
        let wt_root = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-b", "master"]).await.unwrap();
        super::init_repo(repo.path(), "master").await.unwrap();

        // The project row still says "main" — a rename, or a default guessed
        // before the repo existed.
        let mgr = WorktreeManager::new(wt_root.path());
        let wt = mgr
            .create(repo.path(), "main", Uuid::new_v4(), "task")
            .await
            .expect("should branch from the repo's actual branch");
        assert!(wt.path.exists());
    }

    /// Projects added before commits were checked for are still in the
    /// database, so starting a run has to repair them rather than fail.
    #[tokio::test]
    async fn a_run_on_a_commitless_repo_repairs_it_instead_of_failing() {
        let repo = tempfile::tempdir().unwrap();
        let wt_root = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-b", "main"]).await.unwrap();
        tokio::fs::write(repo.path().join("main.py"), "x = 1\n")
            .await
            .unwrap();

        let mgr = WorktreeManager::new(wt_root.path());
        let wt = mgr
            .create(repo.path(), "main", Uuid::new_v4(), "software-team")
            .await
            .expect("a commitless repo should be repaired, not rejected");
        assert!(wt.path.join("main.py").exists());
    }

    #[tokio::test]
    async fn resolve_base_explains_a_repo_with_nothing_to_branch_from() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-b", "main"]).await.unwrap();
        let err = resolve_base(repo.path(), "main").await.unwrap_err().to_string();
        assert!(err.contains("no commits"), "unhelpful message: {err}");
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
