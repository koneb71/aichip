//! Which files of a repository are worth indexing.
//!
//! `git ls-files` rather than a directory walk, and that is the whole trick:
//! it honours `.gitignore` for free, so `node_modules`, `target`, `dist` and
//! every build artifact are excluded by the rules the project already wrote
//! down, and it never descends into them to find that out. The walk in
//! `rag::index` honours no ignore file — fine for a managed space folder,
//! ruinous on a repository, where `node_modules` alone would be more files
//! than the source.
//!
//! A project with no repository falls back to the walk, bounded by the same
//! skip list the file browser uses.

use std::path::Path;

/// Never useful to index and large enough that walking them hurts. The same
/// three `routes/files.rs` skips, for the same reasons.
const SKIP_DIRS: [&str; 6] = [".git", "node_modules", "target", "dist", "build", ".venv"];

/// Bigger than this and it is a bundle, a lockfile or a fixture, not source
/// somebody wants to find by meaning. Matches the file browser's cap.
pub const MAX_FILE_BYTES: u64 = 512 * 1024;

/// Extensions worth reading. A closed list, deliberately: the point is to
/// index what a person writes, not every blob in the tree. Lockfiles,
/// minified bundles and generated clients are the noise this keeps out.
const CODE_EXTENSIONS: [&str; 24] = [
    "rs", "ts", "tsx", "js", "jsx", "mjs", "py", "go", "rb", "java", "kt", "swift", "c", "h",
    "cc", "cpp", "hpp", "cs", "php", "sh", "sql", "toml", "yaml", "yml",
];

/// Documentation worth indexing beside the code: a README answers "how does
/// this work" more often than any source file.
const DOC_EXTENSIONS: [&str; 3] = ["md", "mdx", "txt"];

/// True when a path is worth reading.
///
/// Pure, so the policy is testable without a repository — and the policy is
/// the part that gets argued about.
pub fn is_indexable(rel_path: &str) -> bool {
    if rel_path
        .split('/')
        .any(|seg| SKIP_DIRS.contains(&seg) || seg.starts_with('.') && seg.len() > 1)
    {
        return false;
    }
    // A generated or minified file is bytes, not writing.
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    // `-lock.` rather than a list of suffixes: pnpm-lock.yaml, package-lock.json
    // and Cargo.lock are all the same kind of thing, and the first of those is
    // a `.yaml`, which is otherwise indexable.
    if name.contains(".min.") || name.ends_with(".lock") || name.contains("-lock.") {
        return false;
    }
    let Some(ext) = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()) else {
        return false;
    };
    CODE_EXTENSIONS.contains(&ext.as_str()) || DOC_EXTENSIONS.contains(&ext.as_str())
}

/// Every indexable file in the project, repo-relative and sorted.
///
/// Sorted because the reconcile's set-difference and the index's ordering both
/// want a stable list, and because a diff of two runs should read as a change
/// in the repository rather than in the filesystem's mood.
pub async fn files(root: &Path, vcs_is_git: bool) -> anyhow::Result<Vec<String>> {
    let mut out = if vcs_is_git {
        match tracked(root).await {
            Ok(v) => v,
            // A repository with no commits yet, or a git that failed for its
            // own reasons: fall back rather than report an empty project.
            Err(e) => {
                tracing::debug!(error = %e, "git ls-files failed; walking instead");
                walk(root).await?
            }
        }
    } else {
        walk(root).await?
    };
    out.retain(|p| is_indexable(p));
    out.sort();
    out.dedup();
    Ok(out)
}

/// What git says the project contains.
///
/// `-z` because a path may contain a newline, and `--cached --others
/// --exclude-standard` so a file that exists but has never been committed is
/// still indexed — someone mid-feature should find their own new code.
async fn tracked(root: &Path) -> anyhow::Result<Vec<String>> {
    let out = tokio::process::Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .output()
        .await?;
    if !out.status.success() {
        anyhow::bail!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

/// The fallback for a folder with no repository.
async fn walk(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue; // a folder deleted mid-walk is not an error
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(ft) = entry.file_type().await else { continue };
            // A symlink out of the project is a folder escape; not followed.
            if ft.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(entry.path());
                }
                continue;
            }
            if let Ok(rel) = entry.path().strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_the_languages_this_machine_actually_has() {
        for p in [
            "crates/aichip-core/src/brain.rs",
            "web/src/lib/api.ts",
            "web/src/pages/ProjectPage.tsx",
            "backend/apps/accounts/models.py",
            "crates/aichip-core/migrations/0061_project_index.sql",
            "README.md",
        ] {
            assert!(is_indexable(p), "{p} should be indexed");
        }
    }

    #[test]
    fn skips_what_a_gitignore_would_have_skipped_anyway() {
        for p in [
            "node_modules/react/index.js",
            "target/debug/build/x.rs",
            "web/dist/assets/index-a1b2.js",
            ".git/hooks/pre-commit",
            "web/src/vendor/chart.min.js",
            "pnpm-lock.yaml",
            "package-lock.json",
        ] {
            assert!(!is_indexable(p), "{p} must not be indexed");
        }
    }

    #[test]
    fn skips_binaries_and_the_extensionless() {
        for p in ["web/public/logo.png", "Makefile", "assets/font.woff2", "a.pdf"] {
            assert!(!is_indexable(p), "{p} must not be indexed");
        }
    }

    #[test]
    fn a_dotfile_directory_anywhere_in_the_path_is_skipped() {
        assert!(!is_indexable("web/.next/server/page.js"));
        assert!(!is_indexable(".github/workflows/ci.yml"));
        // …but a leading dot on the *file* is fine to reject too: there is no
        // source worth finding by meaning in a .env.
        assert!(!is_indexable(".env.example"));
    }
}
