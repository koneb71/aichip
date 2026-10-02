//! The product's name, in every form it takes — and the one it had before.
//!
//! Eren was called aichip, and that name is written into state that already
//! exists on people's machines and in their repositories: a home folder whose
//! absolute paths are stored in the database and in git's own worktree links,
//! environment variables set in shell profiles and read by hook scripts, card
//! branches, a managed database, a storage bucket, folders committed to
//! repositories, headers sent by scripts, and app files that call the bridge by
//! its old path.
//!
//! A rename that only rewrote the source would strand all of it. So every old
//! spelling lives **here and nowhere else**: the rest of the code asks this
//! module, and a source-scanning test (`no_old_name_outside_brand`) fails the
//! build when the old name turns up anywhere it is not allowed. When the
//! compatibility window closes, deleting the `LEGACY*` items and following the
//! compile errors removes it completely.
//!
//! The rule for each kind of state is the same: **write the new name, read
//! both, prefer the new.** Nothing a person made is moved or rewritten, with
//! one exception — the home folder, which is Eren's own and is moved once,
//! leaving a link behind (see [`adopt_legacy_home`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The name in identifiers, the binary, paths and the MCP server.
pub const NAME: &str = "eren";
/// The name in prose.
pub const LABEL: &str = "Eren";
/// Every name the product has had, current first.
///
/// For finding things Eren made and named after itself that outlive a restart
/// — preview containers, their images and compose stacks. A sweep that only
/// knew the new name would leave everything started before the rename
/// running, holding its port, with nothing left that would ever stop it.
pub const NAMES: &[&str] = &[NAME, "aichip"];

const ENV_PREFIX: &str = "EREN_";
const LEGACY_ENV_PREFIX: &str = "AICHIP_";

// ── Environment ────────────────────────────────────────────────────────────

/// The variable's current name: `env_name("BIND")` is `EREN_BIND`. For
/// messages that tell a person what to set.
pub fn env_name(key: &str) -> String {
    format!("{ENV_PREFIX}{key}")
}

/// Both spellings of a variable, new first.
pub fn env_names(key: &str) -> [String; 2] {
    [env_name(key), format!("{LEGACY_ENV_PREFIX}{key}")]
}

/// A setting from the environment: `EREN_<key>`, else `AICHIP_<key>`.
///
/// Set-but-empty counts as set, exactly as `std::env::var` would have it, so a
/// caller that treats an empty value specially keeps doing so.
pub fn var(key: &str) -> Option<String> {
    env_names(key)
        .into_iter()
        .find_map(|name| std::env::var(name).ok())
}

/// [`var`], for values that need not be UTF-8 (paths).
pub fn var_os(key: &str) -> Option<OsString> {
    env_names(key).into_iter().find_map(std::env::var_os)
}

/// Variables still set under the old name, each with the name to use now,
/// for a note at boot.
///
/// Any `AICHIP_*` at all is a setting meant for Eren — nothing else uses the
/// prefix. Sorted, so the note reads the same every time.
pub fn legacy_env_in_use() -> Vec<(String, String)> {
    let mut names: Vec<(String, String)> = std::env::vars_os()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter_map(|k| {
            let key = k.strip_prefix(LEGACY_ENV_PREFIX)?.to_string();
            Some((k, env_name(&key)))
        })
        .collect();
    names.sort();
    names
}

/// Variables for a spawned process, each `EREN_*` one also under its old name.
///
/// Scripts people wrote — an MCP server, an engine's own hook, anything an agent
/// runs — read `$AICHIP_RUN_ID` and the rest; setting both for the
/// compatibility window keeps them working while new ones are written against
/// `EREN_*`.
pub fn with_legacy_env(
    mut env: std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    let old: Vec<(String, String)> = env
        .iter()
        .filter_map(|(k, v)| Some((legacy_env_name(k)?, v.clone())))
        .collect();
    env.extend(old);
    env
}

/// The old spelling of a variable given its current one: `EREN_TITLE` →
/// `AICHIP_TITLE`. `None` for a name that is not Eren's.
pub fn legacy_env_name(current: &str) -> Option<String> {
    current
        .strip_prefix(ENV_PREFIX)
        .map(|key| format!("{LEGACY_ENV_PREFIX}{key}"))
}

// ── The home folder ────────────────────────────────────────────────────────

const HOME_DIR: &str = ".eren";
const LEGACY_HOME_DIR: &str = ".aichip";

fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `~/.eren`: worktrees, attachments, apps, previews, the managed database.
pub fn home() -> PathBuf {
    user_home().join(HOME_DIR)
}

/// What [`adopt_legacy_home`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Adoption {
    /// No old folder, or it is already a link: nothing to do.
    Nothing,
    /// The old folder was moved to the new place and a link left behind.
    Moved { from: PathBuf, to: PathBuf },
    /// Both exist as real folders. Left alone: merging two homes is not
    /// something to guess at, and the new one is what gets used.
    Both { legacy: PathBuf },
}

/// Move `~/.aichip` to `~/.eren`, once, and leave a link at the old path.
///
/// The link is the point. The database stores absolute paths into the home
/// folder (a card's worktree, an app's folder), and git records each
/// worktree's absolute path in the repository it belongs to. Rewriting all of
/// that would mean editing files inside people's repositories; a link makes
/// every one of those paths keep resolving with nothing rewritten.
///
/// Called first thing by `serve`, before the managed Postgres starts, so no
/// file in the folder is open. A rename within one directory is atomic, so an
/// interruption leaves either the old layout or the new one, never half. If the
/// link cannot be made, the move is undone and the error returned: a moved
/// folder without its link would break every stored path, which is worse than
/// not starting.
pub fn adopt_legacy_home() -> std::io::Result<Adoption> {
    adopt_in(&user_home())
}

fn adopt_in(user_home: &Path) -> std::io::Result<Adoption> {
    let new = user_home.join(HOME_DIR);
    let old = user_home.join(LEGACY_HOME_DIR);
    match std::fs::symlink_metadata(&old) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Adoption::Nothing),
        Err(e) => return Err(e),
        Ok(m) if m.file_type().is_symlink() => return Ok(Adoption::Nothing),
        Ok(_) => {}
    }
    if std::fs::symlink_metadata(&new).is_ok() {
        return Ok(Adoption::Both { legacy: old });
    }
    std::fs::rename(&old, &new)?;
    // Relative, so the link survives the home directory itself moving.
    if let Err(e) = link(Path::new(HOME_DIR), &old) {
        std::fs::rename(&new, &old)?;
        return Err(e);
    }
    Ok(Adoption::Moved { from: old, to: new })
}

#[cfg(unix)]
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(not(unix))]
fn link(_target: &Path, _at: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "moving the old home folder needs a symlink",
    ))
}

// ── Names written into repositories ────────────────────────────────────────

/// The prefix of every card branch Eren creates.
pub const BRANCH_PREFIX: &str = "eren/";
/// Every prefix a card branch may carry. Branches made before the rename are
/// still in people's repositories, and still belong to cards.
pub const BRANCH_PREFIXES: &[&str] = &[BRANCH_PREFIX, "aichip/"];

/// Whether a branch is one Eren made for a card.
pub fn is_card_branch(branch: &str) -> bool {
    BRANCH_PREFIXES.iter().any(|p| branch.starts_with(p))
}

/// The folder Eren reads inside a project: `.eren/workflows`, `.eren/apps`.
pub const REPO_DIR: &str = ".eren";
/// Every such folder, in the order to look. A repository that committed the
/// old one keeps it; nothing here moves files in someone's repository.
pub const REPO_DIRS: &[&str] = &[REPO_DIR, ".aichip"];

/// The first of [`REPO_DIRS`] under `project` that exists, joined with `sub`;
/// the new name when none does.
pub fn repo_dir(project: &Path, sub: &str) -> PathBuf {
    REPO_DIRS
        .iter()
        .map(|d| project.join(d).join(sub))
        .find(|p| p.is_dir())
        .unwrap_or_else(|| project.join(REPO_DIR).join(sub))
}

/// An app's manifest file.
pub const APP_MANIFEST: &str = "eren.app.yaml";
/// Every manifest name, in the order to look.
pub const APP_MANIFESTS: &[&str] = &[APP_MANIFEST, LEGACY_APP_MANIFEST];
/// The manifest an app made before the rename carries. Eren's own app folders
/// are renamed at boot; one in somebody's repository is only read.
pub const LEGACY_APP_MANIFEST: &str = "aichip.app.yaml";

/// The manifest in `dir`: whichever of [`APP_MANIFESTS`] exists, else the new
/// name.
pub fn app_manifest(dir: &Path) -> PathBuf {
    APP_MANIFESTS
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.is_file())
        .unwrap_or_else(|| dir.join(APP_MANIFEST))
}

/// The `kind` an exported app bundle declares.
pub const BUNDLE_KIND: &str = "eren-app";
/// Every `kind` an importer accepts. A bundle is a file someone saved; one
/// exported before the rename is still a bundle.
pub const BUNDLE_KINDS: &[&str] = &[BUNDLE_KIND, "aichip-app"];

// ── Names on the wire ──────────────────────────────────────────────────────

/// The MCP server name engines are told, and so the `mcp__eren__` in every
/// tool name.
pub const MCP_SERVER: &str = NAME;
/// The prefix of every tool Eren serves.
pub const MCP_TOOL_PREFIX: &str = "mcp__eren__";
/// The whole server as one entry in an allow-list: every tool it serves.
pub const MCP_SERVER_TOOLS: &str = "mcp__eren";
const LEGACY_MCP_SERVER_TOOLS: &str = "mcp__aichip";

/// `mcp__eren__<name>`.
pub fn mcp_tool(name: &str) -> String {
    format!("{MCP_TOOL_PREFIX}{name}")
}

/// A tool name with the old server name rewritten to the new one.
///
/// For lists stored or written before the rename — an agent's allowed tools,
/// a template someone saved. Migration 0088 rewrites the rows already in the
/// database; this catches whatever arrives later from a file. An entry naming
/// `mcp__aichip__list_tasks` must still mean `mcp__eren__list_tasks`, or a
/// rename would quietly take a tool away.
pub fn tool_name(name: &str) -> std::borrow::Cow<'_, str> {
    match name.strip_prefix(LEGACY_MCP_SERVER_TOOLS) {
        Some("") => std::borrow::Cow::Owned(MCP_SERVER_TOOLS.to_string()),
        Some(rest) if rest.starts_with("__") => {
            std::borrow::Cow::Owned(format!("{MCP_SERVER_TOOLS}{rest}"))
        }
        _ => std::borrow::Cow::Borrowed(name),
    }
}

/// [`tool_name`] over a list, in place.
pub fn rename_tools(tools: &mut [String]) {
    for t in tools.iter_mut() {
        if let std::borrow::Cow::Owned(renamed) = tool_name(t) {
            *t = renamed;
        }
    }
}

/// The header a dashboard write carries (see `eren_server::routes`).
pub const WRITE_HEADER: &str = "x-eren-write";
/// The header every app bridge call carries.
pub const APP_HEADER: &str = "x-eren-app";
/// Both header pairs, new first. Scripts written against the old API send the
/// old names; the header's only job is to be one a cross-origin page cannot
/// set, which either spelling does equally.
pub const WRITE_HEADERS: &[&str] = &[WRITE_HEADER, "x-aichip-write"];
pub const APP_HEADERS: &[&str] = &[APP_HEADER, "x-aichip-app"];

/// The path an app reaches the bridge on: `/__eren/…`.
pub const BRIDGE_PREFIX: &str = "__eren";
/// Every bridge prefix. Apps built before the rename load
/// `/__aichip/client.js` and call `window.aichip`, from files in their own
/// folders that Eren does not rewrite.
pub const BRIDGE_PREFIXES: &[&str] = &[BRIDGE_PREFIX, LEGACY_BRIDGE_PREFIX];
/// The old prefix, and so the old global `client.js` also defines.
pub const LEGACY_BRIDGE_PREFIX: &str = "__aichip";
pub const LEGACY_APP_GLOBAL: &str = "aichip";

// ── Names of stored data ───────────────────────────────────────────────────

/// The managed Postgres database.
pub const DATABASE: &str = NAME;
/// The name it had, renamed in place at boot.
pub const LEGACY_DATABASE: &str = "aichip";

/// The object-storage bucket used when none is configured.
pub const BUCKET: &str = NAME;
/// The default bucket before the rename. Used instead of [`BUCKET`] when it
/// exists and the new one does not, so stored attachments stay reachable.
pub const LEGACY_BUCKET: &str = "aichip";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variable_has_two_names_and_the_new_one_comes_first() {
        assert_eq!(env_name("BIND"), "EREN_BIND");
        assert_eq!(env_names("BIND"), ["EREN_BIND", "AICHIP_BIND"]);
    }

    #[test]
    fn the_new_name_wins_and_the_old_one_still_counts() {
        // Unique keys, so no other test races these.
        let [new, old] = env_names("BRAND_TEST_PREFERS");
        std::env::set_var(&old, "old");
        assert_eq!(var("BRAND_TEST_PREFERS").as_deref(), Some("old"));
        std::env::set_var(&new, "new");
        assert_eq!(var("BRAND_TEST_PREFERS").as_deref(), Some("new"));
        assert_eq!(
            var_os("BRAND_TEST_PREFERS").as_deref(),
            Some(std::ffi::OsStr::new("new"))
        );
        assert!(legacy_env_in_use().contains(&(old.clone(), new.clone())));
        std::env::remove_var(&new);
        std::env::remove_var(&old);
        assert_eq!(var("BRAND_TEST_PREFERS"), None);
    }

    #[test]
    fn a_current_name_has_one_old_spelling() {
        assert_eq!(
            legacy_env_name("EREN_TITLE").as_deref(),
            Some("AICHIP_TITLE")
        );
        assert_eq!(legacy_env_name("PATH"), None);
    }

    #[test]
    fn a_child_gets_both_names() {
        let env = with_legacy_env(std::collections::HashMap::from([
            ("EREN_RUN_ID".to_string(), "r1".to_string()),
            ("MCP_TOOL_TIMEOUT".to_string(), "60".to_string()),
        ]));
        assert_eq!(env["EREN_RUN_ID"], "r1");
        assert_eq!(env["AICHIP_RUN_ID"], "r1");
        // Only Eren's own variables gain a second name.
        assert_eq!(env.len(), 3, "{env:?}");
    }

    #[test]
    fn the_old_home_moves_once_and_leaves_a_link() {
        let home = tempfile::tempdir().unwrap();
        let old = home.path().join(LEGACY_HOME_DIR);
        std::fs::create_dir_all(old.join("worktrees/card")).unwrap();
        std::fs::write(old.join("worktrees/card/f"), "kept").unwrap();

        let moved = adopt_in(home.path()).unwrap();
        assert_eq!(
            moved,
            Adoption::Moved {
                from: old.clone(),
                to: home.path().join(HOME_DIR)
            }
        );
        // The data is at the new place…
        let new = home.path().join(HOME_DIR);
        assert_eq!(
            std::fs::read_to_string(new.join("worktrees/card/f")).unwrap(),
            "kept"
        );
        assert!(!std::fs::symlink_metadata(&new)
            .unwrap()
            .file_type()
            .is_symlink());
        // …and a path stored before the move still reaches it.
        assert!(std::fs::symlink_metadata(&old)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_to_string(old.join("worktrees/card/f")).unwrap(),
            "kept"
        );

        // A second boot finds a link and leaves it.
        assert_eq!(adopt_in(home.path()).unwrap(), Adoption::Nothing);
    }

    #[test]
    fn a_fresh_machine_has_nothing_to_adopt() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(adopt_in(home.path()).unwrap(), Adoption::Nothing);
        assert!(!home.path().join(HOME_DIR).exists());
    }

    #[test]
    fn two_real_homes_are_not_merged() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(LEGACY_HOME_DIR)).unwrap();
        std::fs::create_dir(home.path().join(HOME_DIR)).unwrap();
        assert_eq!(
            adopt_in(home.path()).unwrap(),
            Adoption::Both {
                legacy: home.path().join(LEGACY_HOME_DIR)
            }
        );
        assert!(home.path().join(LEGACY_HOME_DIR).is_dir());
    }

    #[test]
    fn a_card_branch_is_recognised_under_either_prefix() {
        assert!(is_card_branch("eren/fix-login-1a2b3c4d"));
        assert!(is_card_branch("aichip/fix-login-1a2b3c4d"));
        assert!(!is_card_branch("main"));
        assert!(!is_card_branch("feature/eren"));
    }

    #[test]
    fn a_repository_folder_is_read_under_either_name_new_first() {
        let repo = tempfile::tempdir().unwrap();
        // Neither: the new name, so a write lands in the right place.
        assert_eq!(
            repo_dir(repo.path(), "workflows"),
            repo.path().join(".eren/workflows")
        );
        std::fs::create_dir_all(repo.path().join(".aichip/workflows")).unwrap();
        assert_eq!(
            repo_dir(repo.path(), "workflows"),
            repo.path().join(".aichip/workflows")
        );
        std::fs::create_dir_all(repo.path().join(".eren/workflows")).unwrap();
        assert_eq!(
            repo_dir(repo.path(), "workflows"),
            repo.path().join(".eren/workflows")
        );
    }

    #[test]
    fn an_app_manifest_is_found_under_either_name() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(app_manifest(dir.path()), dir.path().join(APP_MANIFEST));
        std::fs::write(dir.path().join(LEGACY_APP_MANIFEST), "name: T").unwrap();
        assert_eq!(
            app_manifest(dir.path()),
            dir.path().join(LEGACY_APP_MANIFEST)
        );
        std::fs::write(dir.path().join(APP_MANIFEST), "name: T").unwrap();
        assert_eq!(app_manifest(dir.path()), dir.path().join(APP_MANIFEST));
    }

    #[test]
    fn an_old_tool_name_still_means_the_same_tool() {
        assert_eq!(
            tool_name("mcp__aichip__start_task"),
            "mcp__eren__start_task"
        );
        assert_eq!(tool_name("mcp__eren__start_task"), "mcp__eren__start_task");
        assert_eq!(tool_name("Bash"), "Bash");
        // The whole server, as an allow-list entry, is renamed too…
        assert_eq!(tool_name("mcp__aichip"), "mcp__eren");
        // …but a different server that merely starts the same way is not.
        assert_eq!(tool_name("mcp__aichipper__x"), "mcp__aichipper__x");
        let mut list = vec![
            "Read".to_string(),
            "mcp__aichip__comment".to_string(),
            "mcp__aichip".to_string(),
        ];
        rename_tools(&mut list);
        assert_eq!(list, ["Read", "mcp__eren__comment", "mcp__eren"]);
    }

    #[test]
    fn the_current_name_is_first_in_every_list() {
        // Writers use the first; readers try them in order. Getting the order
        // wrong would write the old name back.
        assert_eq!(BRANCH_PREFIXES[0], BRANCH_PREFIX);
        assert_eq!(REPO_DIRS[0], REPO_DIR);
        assert_eq!(APP_MANIFESTS[0], APP_MANIFEST);
        assert_eq!(BUNDLE_KINDS[0], BUNDLE_KIND);
        assert_eq!(WRITE_HEADERS[0], WRITE_HEADER);
        assert_eq!(APP_HEADERS[0], APP_HEADER);
        assert_eq!(BRIDGE_PREFIXES[0], BRIDGE_PREFIX);
        assert_eq!(NAMES[0], NAME);
        assert_eq!(MCP_TOOL_PREFIX, format!("mcp__{MCP_SERVER}__"));
        assert_eq!(MCP_SERVER_TOOLS, format!("mcp__{MCP_SERVER}"));
    }
}

/// The old name stays where it belongs.
///
/// Read across the whole repository — Rust, the dashboard, configuration and
/// documents — in the same spirit as `env_guard`'s scan for `Command::new`:
/// a rename is only finished if nothing can quietly reintroduce the name it
/// replaced, and review alone does not catch a string.
#[cfg(test)]
mod old_name_scan {
    use std::path::Path;

    /// Files whose job is the old name: the compatibility code and its tests,
    /// migration history (never edited), and the configuration naming data
    /// that already exists on people's machines.
    const ALLOWED_FILES: &[&str] = &[
        "crates/eren-shared/src/brand.rs",
        "crates/eren-core/src/legacy.rs",
        "web/src/lib/brand.ts",
        "web/src/lib/brand.test.ts",
        // Volume names and Postgres defaults: they name existing data.
        "docker-compose.yml",
        ".env.example",
        // The link from the old state path to the new one.
        "Dockerfile",
        // The theme, read on the one load before brand.ts moves it.
        "web/index.html",
    ];
    const ALLOWED_DIRS: &[&str] = &["crates/eren-core/migrations/"];
    /// Generated, and regenerated by tools that know nothing of this test.
    const SKIPPED: &[&str] = &["Cargo.lock", "web/pnpm-lock.yaml"];

    fn old() -> String {
        // Assembled, so this file's own source is not what the scan finds
        // first if the allow-list above is ever wrong.
        ["ai", "chip"].concat()
    }

    /// Where the old name is found and should not be: `path:line`.
    fn offenders(root: &Path, files: &[String]) -> Vec<String> {
        let old = old();
        let mut found = vec![];
        for file in files {
            if ALLOWED_FILES.contains(&file.as_str())
                || SKIPPED.contains(&file.as_str())
                || ALLOWED_DIRS.iter().any(|d| file.starts_with(d))
            {
                continue;
            }
            // Images, fonts: not text, and not where a name creeps back in.
            let Ok(text) = std::fs::read_to_string(root.join(file)) else {
                continue;
            };
            let prose = file.ends_with(".md");
            for (n, line) in text.lines().enumerate() {
                let lower = line.to_lowercase();
                if !lower.contains(&old) {
                    continue;
                }
                // A document may say what Eren used to be called, and an
                // upgrade note has to — but only beside the name it is now,
                // so a stale `cargo run -p <old>-cli` cannot hide as history.
                if prose && lower.contains(super::NAME) {
                    continue;
                }
                found.push(format!("{file}:{}: {}", n + 1, line.trim()));
            }
        }
        found
    }

    #[tokio::test]
    async fn no_old_name_outside_brand() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // Tracked files and new ones not ignored: what a commit would carry,
        // and never somebody's own `.env`.
        let Ok(out) = crate::env_guard::command("git")
            .args(["ls-files", "--cached", "--others", "--exclude-standard"])
            .current_dir(&root)
            .output()
            .await
        else {
            eprintln!("skipped: git is not available to list the repository");
            return;
        };
        let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect();
        assert!(files.len() > 100, "git listed {} files", files.len());
        let found = offenders(&root, &files);
        assert!(
            found.is_empty(),
            "the old name belongs in eren_shared::brand (or web/src/lib/brand.ts) \
             and nowhere else — ask brand for it instead:\n{}",
            found.join("\n")
        );
    }

    #[test]
    fn the_scan_finds_what_it_is_for() {
        let dir = tempfile::tempdir().unwrap();
        let old = old();
        let write = |name: &str, text: String| {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("src/a.rs", format!("let dir = \".{old}\";\n"));
        write("README.md", format!("cargo run -p {old}-cli\n"));
        write("NOTES.md", format!("Eren was called {old}.\n"));
        write(
            "crates/eren-core/migrations/0001.sql",
            format!("-- {old}\n"),
        );
        let files: Vec<String> = ["src/a.rs", "README.md", "NOTES.md"]
            .into_iter()
            .chain(["crates/eren-core/migrations/0001.sql"])
            .map(str::to_string)
            .collect();
        let found = offenders(dir.path(), &files);
        // Code is caught, and a stale command in a document; history and a
        // sentence about the rename are not.
        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].starts_with("src/a.rs:1"));
        assert!(found[1].starts_with("README.md:1"));
    }
}
