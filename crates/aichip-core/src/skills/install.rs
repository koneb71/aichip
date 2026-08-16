//! Installing a skill from a registry, and mirroring it into the library.
//!
//! Two halves, because the user asked for both and they answer different
//! questions.
//!
//! **Install** runs `npx skills add owner/repo` in the project. What lands is
//! a real Agent Skill: `.agents/skills/<name>/` with its `SKILL.md` and
//! whatever it bundles, symlinked into `.claude/skills/` so Claude Code reads
//! it natively and copied to `agent/skills/` for the agents that look there.
//! That is the copy with full fidelity — a skill shipping `resources/deploy.sh`
//! still has its script.
//!
//! **Mirror** copies each `SKILL.md`'s frontmatter and body into a `skills`
//! row, so the same skill can be `@name`d in a chat, bound to a card, and
//! carried to an engine that has never heard of the format.
//!
//! ## Which copy wins
//!
//! The folder. Always. The row is stamped with the installer's own content
//! hash and re-derived from disk on every install and every sync; an edit made
//! to the row in aichip is overwritten the next time either happens. That is
//! stated in the UI rather than enforced, because the escape hatch — copy it
//! into a skill of your own and edit that — is one a person can find, and
//! refusing the edit outright would be a worse answer than losing it.
//!
//! ## Two things this deliberately does not do
//!
//! **It never installs globally.** `npx skills add -g` writes to
//! `~/.claude/skills`, and the second compliance invariant is that aichip does
//! not touch `~/.claude` — it is the engine's own directory, and the whole
//! reason `doctor` decides "is this logged in?" by *running* the CLI rather
//! than reading its files. Project scope is not a limitation here, it is the
//! only scope aichip is allowed to have.
//!
//! **It does not read the installer's stdout for what happened.** That output
//! is a spinner and a box-drawn table full of ANSI escapes. `skills-lock.json`
//! is the manifest the tool writes for exactly this purpose, and reading the
//! file it wrote beats parsing the animation it drew.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use sqlx::Row;
use tokio::process::Command;
use uuid::Uuid;

use super::registry::{self, LockEntry};
use crate::db::Db;

/// Where the installer puts the canonical copy.
const CANONICAL: &str = ".agents/skills";
/// The manifest it writes beside it.
const LOCKFILE: &str = "skills-lock.json";

/// What one install produced.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub skills: Vec<InstalledSkill>,
    /// Whether the result was committed. A worktree is branched from HEAD, so
    /// an uncommitted `.claude/skills` never reaches a card run at all — the
    /// same failure `apps::commit` exists to prevent, which cost a paid run
    /// before it did.
    pub committed: bool,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub source: String,
    /// Files that came with it which are not markdown — the scripts and data
    /// an agent may execute. Surfaced because the installer's own parting
    /// words are "review skills before use; they run with full agent
    /// permissions", and a list of what arrived is the only version of that
    /// advice a person can act on.
    pub bundled: Vec<String>,
    /// The library row, when the skill could be mirrored.
    pub skill_id: Option<Uuid>,
    /// Why it was not mirrored, when it was not. A skill that installed fine
    /// but could not be mirrored is a partial success, not a failure — the
    /// files are there and the engine will read them.
    pub mirror_error: Option<String>,
}

/// Run the installer in a project, then mirror what landed.
pub async fn install(db: &Db, project_id: Uuid, reference: &str) -> anyhow::Result<Installed> {
    let reference = registry::normalise_ref(reference).map_err(|e| anyhow::anyhow!(e))?;

    let row = sqlx::query("SELECT path, workspace_id, kind FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(&db.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no such project"))?;
    if row.get::<String, _>("kind") == "space" {
        anyhow::bail!("a document space has no agents to install a skill for");
    }
    let root = PathBuf::from(row.get::<String, _>("path"));
    let workspace_id: Uuid = row.get("workspace_id");

    run_installer(&root, &reference).await?;

    // What the tool says it installed, rather than what it drew on screen.
    let lock_path = root.join(LOCKFILE);
    let lock = tokio::fs::read_to_string(&lock_path).await.map_err(|e| {
        anyhow::anyhow!("the installer wrote no {LOCKFILE}, so nothing can be read back: {e}")
    })?;
    let entries = registry::parse_lock(&lock).map_err(|e| anyhow::anyhow!(e))?;

    let mut out = Installed::default();
    for entry in entries.iter().filter(|e| e.source == reference) {
        out.skills.push(mirror_one(db, workspace_id, project_id, &root, entry).await);
    }
    if out.skills.is_empty() {
        anyhow::bail!("{reference} installed no skills — check the repository has a skills folder");
    }

    // Committed, not merely written. See `Installed::committed`.
    out.committed = crate::worktrees::manager::commit_all(
        &root,
        &format!("Add agent skills from {reference}"),
    )
    .await
    .unwrap_or(false);
    Ok(out)
}

/// Spawn the installer. Never `-g`; never a shell string.
async fn run_installer(root: &Path, reference: &str) -> anyhow::Result<()> {
    let mut cmd = Command::new("npx");
    cmd.args([
        // Pinned to the package name rather than a version: this is somebody
        // else's release train and following it is the point.
        "-y",
        "skills@latest",
        "add",
        reference,
        // Every skill in the repository, without a prompt. There is no console
        // here, so an interactive picker is not a slow answer — it is a
        // process that never returns.
        "--all",
        "--yes",
    ])
    .current_dir(root)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    // No colour and no spinner in a pipe, which keeps the error legible when
    // there is one.
    .env("NO_COLOR", "1")
    .env("CI", "1")
    .kill_on_drop(true);
    // A spawned child never inherits aichip's own secrets — the same rule the
    // engines and `gh` apply, applied to the fifth CLI.
    for key in aichip_shared::env_guard::AICHIP_OWN_SECRETS {
        cmd.env_remove(key);
    }

    let out = match cmd.output().await {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("npx isn't on this machine — install Node to add skills from a registry")
        }
        Err(e) => anyhow::bail!("could not run the skills installer: {e}"),
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let detail = [stderr.trim(), stdout.trim()]
            .iter()
            .find(|s| !s.is_empty())
            .map(|s| strip_ansi(s))
            .unwrap_or_else(|| "it failed and said nothing".to_string());
        anyhow::bail!("{reference} did not install: {}", clip(&detail, 400));
    }
    Ok(())
}

/// Read one installed skill and upsert its mirror row.
async fn mirror_one(
    db: &Db,
    workspace_id: Uuid,
    project_id: Uuid,
    root: &Path,
    entry: &LockEntry,
) -> InstalledSkill {
    let dir = root.join(CANONICAL).join(&entry.name);
    let mut out = InstalledSkill {
        name: entry.name.clone(),
        description: String::new(),
        source: entry.source.clone(),
        bundled: bundled_files(&dir).await,
        skill_id: None,
        mirror_error: None,
    };

    let doc = match tokio::fs::read_to_string(dir.join("SKILL.md")).await {
        Ok(text) => match registry::parse_skill_md(&text) {
            Ok(doc) => doc,
            Err(e) => {
                out.mirror_error = Some(e);
                return out;
            }
        },
        Err(e) => {
            out.mirror_error = Some(format!("could not read its SKILL.md: {e}"));
            return out;
        }
    };
    out.description = doc.description.clone();

    // The `@` namespace is shared with agents, and a collision is refused in
    // both directions. Nothing in the database enforces that — it is an
    // expression index on skills alone — so an upsert that skipped this check
    // would happily create a skill named after an existing agent and leave
    // every mention of that name ambiguous. The skill is still installed on
    // disk and the engine still reads it; only the mirror is declined.
    match super::check_name_free(db, workspace_id, &doc.name, None).await {
        Ok(Ok(())) => {}
        Ok(Err(why)) => {
            out.mirror_error = Some(format!("installed, but not added to the library — {why}"));
            return out;
        }
        Err(e) => {
            out.mirror_error = Some(format!("installed, but the library could not be checked: {e}"));
            return out;
        }
    }

    // Deliberately *not* run through `looks_like_secret`, which every
    // hand-written skill is checked against on save. That check exists so a
    // person does not paste their own credential into aichip's database. This
    // text is a public file that is already sitting in the repository on disk,
    // committed; refusing to mirror it would not remove a single byte, it
    // would only make the library disagree with what the engine reads. And the
    // skills most worth having are the ones about deploying with tokens, which
    // is exactly what that check fires on.

    // Upsert on the name the library already uniques by, so installing twice
    // updates rather than colliding. `enabled` is left alone on conflict: a
    // skill somebody deliberately turned off must not come back on because
    // the installer ran again.
    let res = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO skills
            (workspace_id, name, description, instructions,
             source_repo, source_path, source_hash, source_project_id, installed_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8, now())
         ON CONFLICT (workspace_id, lower(name)) DO UPDATE SET
             description = EXCLUDED.description,
             instructions = EXCLUDED.instructions,
             source_repo = EXCLUDED.source_repo,
             source_path = EXCLUDED.source_path,
             source_hash = EXCLUDED.source_hash,
             source_project_id = EXCLUDED.source_project_id,
             installed_at = now()
         RETURNING id",
    )
    .bind(workspace_id)
    .bind(&doc.name)
    .bind(&doc.description)
    .bind(&doc.body)
    .bind(&entry.source)
    .bind(&entry.skill_path)
    .bind(&entry.hash)
    .bind(project_id)
    .fetch_one(&db.pool)
    .await;

    match res {
        Ok(id) => out.skill_id = Some(id),
        // The most likely cause by far: an agent already owns that name. The
        // `@` namespace is shared, and a collision is refused both ways.
        Err(e) => {
            out.mirror_error = Some(format!(
                "installed, but not added to the library — {}",
                clip(&e.to_string(), 200)
            ))
        }
    }
    out
}

/// The non-markdown files a skill brought with it, relative to its directory.
///
/// Two levels deep, which covers the `resources/`, `scripts/` and
/// `references/` layout real skills use without walking a repository somebody
/// vendored into one.
async fn bundled_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((at, depth)) = stack.pop() {
        let Ok(mut rd) = tokio::fs::read_dir(&at).await else { continue };
        while let Ok(Some(e)) = rd.next_entry().await {
            let path = e.path();
            let Ok(ft) = e.file_type().await else { continue };
            if ft.is_dir() {
                if depth < 2 {
                    stack.push((path, depth + 1));
                }
            } else if path.extension().is_none_or(|x| x != "md") {
                if let Ok(rel) = path.strip_prefix(dir) {
                    out.push(rel.to_string_lossy().to_string());
                }
            }
        }
    }
    out.sort();
    out
}

/// Drop ANSI escapes so an error reads as a sentence.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Skip to the end of the sequence: a letter terminates CSI.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_installers_spinner_out_of_an_error() {
        // Real output: the tool draws a spinner and a box, and an error
        // wrapped in escapes reads as line noise in the UI.
        let raw = "\u{1b}[1G\u{1b}[J\u{1b}[31m◒\u{1b}[0m  could not resolve owner/repo";
        assert_eq!(strip_ansi(raw), "◒  could not resolve owner/repo");
    }

    #[test]
    fn clipping_is_by_characters_not_bytes() {
        // The installer's output is full of box-drawing and spinner glyphs;
        // slicing by byte would panic mid-character.
        let s = "◒◐◓◑".repeat(10);
        let out = clip(&s, 5);
        assert_eq!(out.chars().count(), 6, "five chars and the ellipsis");
    }

    #[test]
    fn a_short_message_is_left_alone() {
        assert_eq!(clip("  fine  ", 100), "fine");
    }
}
