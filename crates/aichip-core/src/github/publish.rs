//! Turn a project that only exists on this disk into one that exists on GitHub.
//!
//! Every other GitHub feature — pull requests, issue import, check status —
//! reads `origin`, so a project that never had one is permanently dark: the
//! card drawer says *"this project has no GitHub `origin` remote, so there is
//! nowhere to open a pull request"* and there was nothing to do about it from
//! inside the product. This is the missing first step.
//!
//! Compliance: `gh repo create` under the user's own login, through the same
//! runner every other call uses. aichip holds no credential and asks for none.

use crate::db::Db;
use crate::github::{gh, repo::parse_repo_ref, GhError};
use crate::worktrees::manager;
use std::path::Path;
use uuid::Uuid;

/// Who may see it. No default — see [`publish`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

impl Visibility {
    fn flag(self) -> &'static str {
        match self {
            Visibility::Private => "--private",
            Visibility::Public => "--public",
        }
    }

    /// Parsed rather than defaulted, so a client that sends nothing is a
    /// refusal and never an accidentally public repository.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "private" => Some(Visibility::Private),
            "public" => Some(Visibility::Public),
            _ => None,
        }
    }
}

/// A name GitHub will take, and a name that cannot be an argument.
///
/// The charset is the defence, exactly as in `parse_repo_ref` and the app
/// manifest: this string becomes a positional argument to `gh`, so a leading
/// `-` would be read as a flag. Rejecting rather than rewriting — silently
/// publishing under a name nobody chose is worse than saying no.
pub fn check_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("a repository needs a name".into());
    }
    if name.len() > 100 {
        return Err("that name is longer than GitHub allows (100 characters)".into());
    }
    if name == "." || name == ".." {
        return Err(format!("\"{name}\" is not a repository name"));
    }
    if name.starts_with('-') {
        return Err("a repository name cannot start with a dash".into());
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
    {
        return Err(format!(
            "\"{bad}\" is not allowed in a repository name — letters, digits, dot, dash \
             and underscore only"
        ));
    }
    Ok(name.to_string())
}

/// The default name for a project: its folder, cleaned up enough to be legal.
///
/// A suggestion shown before the click, never something applied silently — the
/// person sees `owner/name` and can change it.
pub fn suggest_name(path: &str) -> String {
    let base = Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    cleaned.trim_start_matches(['-', '.']).to_string()
}

/// Why a project cannot be published, in the order worth checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotGit,
    NoCommits,
    AlreadyPublished(String),
    GhMissing,
    Failed(String),
}

impl Refusal {
    pub fn message(&self) -> String {
        match self {
            Refusal::NotGit => "this project has no version control, so there is nothing to \
                 publish — aichip can only push a git repository"
                .into(),
            Refusal::NoCommits => "this project has no commits yet, so there is nothing to \
                 publish. Make one first — aichip will not invent history in a repository \
                 about to go to GitHub."
                .into(),
            Refusal::AlreadyPublished(url) => {
                format!(
                    "this project already has an `origin` remote ({url}), so it is \
                         published already"
                )
            }
            Refusal::GhMissing => "the GitHub CLI (gh) is not installed, so aichip cannot \
                 create a repository. Install it and sign in from Connections."
                .into(),
            Refusal::Failed(e) => e.clone(),
        }
    }
}

/// Create the repository and push to it.
///
/// **Visibility is a parameter with no default.** Publishing is outward-facing
/// and the public case cannot be undone in the way that matters — a push that
/// has been seen has been seen — so it is never inferred. The dashboard
/// pre-selects private and shows the `owner/name` that will be created before
/// the button does anything.
///
/// Guards run in the order a person would hit them, each with its own message,
/// so "install gh" is never the advice given to somebody whose real problem is
/// that their folder has no commits.
pub async fn publish(
    db: &Db,
    project_id: Uuid,
    name: Option<&str>,
    visibility: Visibility,
) -> Result<String, Refusal> {
    let row = sqlx::query_as::<_, (String, String)>("SELECT path, vcs FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&db.pool)
        .await
        .map_err(|e| Refusal::Failed(e.to_string()))?
        .ok_or_else(|| Refusal::Failed("no such project".into()))?;
    let (path, vcs) = row;
    let repo = Path::new(&path);

    if vcs != "git" {
        return Err(Refusal::NotGit);
    }
    // Before anything reaches the network. `gh repo create --push` on a
    // commitless repository makes an empty repository on somebody's account
    // that aichip then cannot explain — and the alternative, writing a commit
    // to fill it, is aichip inventing history, which the clone path already
    // refuses for the same reason.
    if manager::head(repo).await.is_none() {
        return Err(Refusal::NoCommits);
    }
    if let Some(url) = manager::remote_url(repo, "origin").await {
        return Err(Refusal::AlreadyPublished(url));
    }

    let name = match name {
        Some(n) => check_name(n).map_err(Refusal::Failed)?,
        None => check_name(&suggest_name(&path)).map_err(Refusal::Failed)?,
    };

    gh(
        Some(repo),
        &[
            "repo",
            "create",
            &name,
            visibility.flag(),
            "--source=.",
            "--remote=origin",
            "--push",
        ],
    )
    .await
    .map_err(|e| match e {
        GhError::NotInstalled => Refusal::GhMissing,
        other => Refusal::Failed(other.to_string()),
    })?;

    // Read the slug back off the remote git now has, rather than scraping it
    // from `gh`'s output: the remote is what every other feature will read, so
    // recording anything else would be recording a second opinion.
    let slug = manager::remote_url(repo, "origin")
        .await
        .and_then(|url| parse_repo_ref(&url).ok())
        .map(|r| r.slug())
        .ok_or_else(|| {
            Refusal::Failed(
                "the repository was created, but aichip could not read the remote back — \
                 reopen the project and it will resolve itself"
                    .into(),
            )
        })?;

    sqlx::query("UPDATE projects SET github_repo = $2 WHERE id = $1")
        .bind(project_id)
        .bind(&slug)
        .execute(&db.pool)
        .await
        .map_err(|e| Refusal::Failed(e.to_string()))?;

    Ok(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_could_be_a_flag_is_refused() {
        // The same shape `parse_repo_ref` guards: this string is a positional
        // argument to `gh`, and a leading dash is read as an option.
        assert!(check_name("-oProxyCommand=touch /tmp/x").is_err());
        assert!(check_name("--private").is_err());
    }

    #[test]
    fn the_charset_is_the_defence() {
        assert!(check_name("my repo").is_err(), "a space is not allowed");
        assert!(
            check_name("owner/name").is_err(),
            "the owner is not chosen here"
        );
        assert!(check_name("what;rm -rf /").is_err());
        assert!(check_name("..").is_err());
        assert!(check_name("").is_err());
        assert!(check_name(&"a".repeat(101)).is_err());

        assert_eq!(check_name("  windows11  ").unwrap(), "windows11");
        assert_eq!(
            check_name("my-project.v2_final").unwrap(),
            "my-project.v2_final"
        );
    }

    #[test]
    fn a_suggested_name_is_the_folder_made_legal() {
        assert_eq!(suggest_name("/Users/x/Documents/windows11"), "windows11");
        assert_eq!(suggest_name("/Users/x/my project"), "my-project");
        // A leading dot would make a hidden repository name; a leading dash
        // would not be a name at all.
        assert_eq!(suggest_name("/Users/x/.dotfiles"), "dotfiles");
    }

    #[test]
    fn visibility_is_never_inferred() {
        assert_eq!(Visibility::parse("private"), Some(Visibility::Private));
        assert_eq!(Visibility::parse("public"), Some(Visibility::Public));
        // Anything else is a refusal rather than a guess — the guess that
        // would be wrong is the one that publishes somebody's code.
        assert_eq!(Visibility::parse(""), None);
        assert_eq!(Visibility::parse("PUBLIC"), None);
        assert_eq!(Visibility::parse("internal"), None);
    }

    #[test]
    fn each_refusal_says_what_to_do_about_itself() {
        assert!(Refusal::NoCommits.message().contains("Make one first"));
        assert!(Refusal::GhMissing.message().contains("Connections"));
        assert!(Refusal::AlreadyPublished("git@github.com:o/r.git".into())
            .message()
            .contains("o/r"));
    }
}
