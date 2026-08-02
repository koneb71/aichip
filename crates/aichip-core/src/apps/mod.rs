//! Apps: things you install, switch on, and use without leaving the dashboard.
//!
//! Two runtimes behind one manifest. A **module** declares models, views and
//! actions, and aichip's own dashboard renders it — nothing arbitrary executes,
//! so there is no container, no iframe and nothing to approve. A **container**
//! app is the escape hatch for work that genuinely needs code, and carries the
//! apparatus that implies.
//!
//! An app is a *project*, on disk under `~/.aichip/apps/<slug>`. That is what
//! makes this small: worktrees, diffs, the files editor, chat and the run
//! orchestrator already exist and already key on a project, so "change this
//! app" is a card and nothing had to be built twice.
//!
//! Neither runtime ever receives a database connection. An app declares what it
//! wants to store and reaches its rows through aichip, which is what keeps
//! agent-written code away from everything it did not declare.

pub mod manifest;
pub mod scope;

pub use manifest::{Manifest, ManifestError, Runtime};
pub use scope::Scope;

use crate::Db;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The manifest's filename inside an app's folder.
pub const MANIFEST_FILE: &str = "aichip.app.yaml";

/// Where apps live.
///
/// Beside worktrees and attachments rather than among the user's own code: an
/// app is aichip's to create, move and delete, and putting them in someone's
/// projects directory would mean a `git status` somewhere unrelated growing
/// entries nobody asked for.
pub fn root() -> PathBuf {
    match std::env::var("AICHIP_APPS_DIR") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => home().join(".aichip").join("apps"),
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The Postgres schema holding an app's tables.
///
/// A schema per app rather than prefixed tables in `public`, which is what Odoo
/// does for historical reasons. Uninstall becomes `DROP SCHEMA … CASCADE`,
/// export is scoped by construction, and two apps may both declare a `note`.
///
/// Built from the slug, which is already a validated DNS label, with dashes
/// folded to underscores. There is nothing in the result that needs quoting.
pub fn schema_name(slug: &str) -> String {
    let cleaned: String = slug
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    format!("app_{cleaned}")
}

/// A DNS-safe, unique label for an app.
///
/// Named for the app so the address bar says what you are looking at, and
/// suffixed from its id so two apps called "Notes" can coexist. Unlike a
/// preview's slug this is an *identity* — it decides which grants and which
/// tables a request gets — which is why the column is `UNIQUE`.
pub fn slug(name: &str, id: &Uuid) -> String {
    let mut label = String::new();
    let mut last_dash = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            label.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !label.is_empty() {
            label.push('-');
            last_dash = true;
        }
    }
    let trimmed: String = label.trim_matches('-').chars().take(32).collect();
    let trimmed = trimmed.trim_end_matches('-');
    let tail = &id.simple().to_string()[..6];
    if trimmed.is_empty() {
        format!("app-{tail}")
    } else {
        format!("{trimmed}-{tail}")
    }
}

/// The hash a manifest is recognised by.
///
/// Used to tell whether the copy in the folder and the copy in the database
/// have drifted — which they can, because a person may edit either.
pub fn digest(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// An app, as everything outside this module sees it.
#[derive(Debug, Clone)]
pub struct App {
    pub id: Uuid,
    pub project_id: Uuid,
    pub workspace_id: Uuid,
    pub slug: String,
    pub name: String,
    pub icon: String,
    pub summary: String,
    pub brief: String,
    pub runtime: Runtime,
    pub active: bool,
    pub schema: String,
    pub manifest: String,
    pub path: PathBuf,
}

impl App {
    /// The parsed manifest.
    ///
    /// Reparsed rather than cached in the struct: the text is the record, and a
    /// stale parse would be a second source of truth for what the app is.
    pub fn parsed(&self) -> Result<Manifest, ManifestError> {
        manifest::parse(&self.manifest)
    }
}

fn row_to_app(r: &sqlx::postgres::PgRow) -> App {
    use sqlx::Row;
    App {
        id: r.get("id"),
        project_id: r.get("project_id"),
        workspace_id: r.get("workspace_id"),
        slug: r.get("slug"),
        name: r.get("name"),
        icon: r.get("icon"),
        summary: r.get("summary"),
        brief: r.get("brief"),
        runtime: Runtime::parse(r.get::<String, _>("runtime").as_str()).unwrap_or(Runtime::Module),
        active: r.get::<String, _>("state") == "active",
        schema: r.get("schema_name"),
        manifest: r.get("manifest"),
        path: PathBuf::from(r.get::<String, _>("path")),
    }
}

const SELECT_APP: &str = "SELECT a.id, a.project_id, a.workspace_id, a.slug, a.name, a.icon,
        a.summary, a.brief, a.runtime, a.state, a.schema_name, a.manifest, p.path
   FROM apps a JOIN projects p ON p.id = a.project_id";

pub async fn list(db: &Db, workspace_id: Option<Uuid>) -> anyhow::Result<Vec<App>> {
    let rows = sqlx::query(&format!(
        "{SELECT_APP} WHERE $1::uuid IS NULL OR a.workspace_id = $1
          ORDER BY a.name ASC"
    ))
    .bind(workspace_id)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows.iter().map(row_to_app).collect())
}

pub async fn get(db: &Db, id: Uuid) -> anyhow::Result<Option<App>> {
    let row = sqlx::query(&format!("{SELECT_APP} WHERE a.id = $1"))
        .bind(id)
        .fetch_optional(&db.pool)
        .await?;
    Ok(row.as_ref().map(row_to_app))
}

pub async fn by_slug(db: &Db, slug: &str) -> anyhow::Result<Option<App>> {
    let row = sqlx::query(&format!("{SELECT_APP} WHERE a.slug = $1"))
        .bind(slug)
        .fetch_optional(&db.pool)
        .await?;
    Ok(row.as_ref().map(row_to_app))
}

/// Create an app from a manifest, folder and all.
///
/// The manifest is validated *before* anything is written, so a manifest an
/// agent got wrong leaves no directory, no project row and no repository
/// behind — a half-made app is worse than none, because it is something a
/// person then has to notice and clean up.
pub async fn install(
    db: &Db,
    workspace_id: Uuid,
    manifest_text: &str,
    brief: &str,
) -> anyhow::Result<App> {
    let parsed = manifest::parse(manifest_text)?;

    let id = Uuid::new_v4();
    let slug = slug(&parsed.name, &id);
    let path = root().join(&slug);
    let schema = schema_name(&slug);

    tokio::fs::create_dir_all(&path).await?;

    // A repository, for the same reason every project has one: a run edits in a
    // worktree, and the worktree is what makes the change reviewable and
    // revertible. Refused nesting is reported now rather than at the end of a
    // generation run someone paid for.
    let repo = crate::worktrees::manager::ensure_repo(&path, "main").await;
    if let crate::worktrees::manager::Vcs::None(reason) = repo {
        let _ = tokio::fs::remove_dir_all(&path).await;
        anyhow::bail!(
            "an app needs its own repository and this folder cannot have one — {reason}. \
             Set AICHIP_APPS_DIR to somewhere outside any repository."
        );
    }

    write_manifest(&path, manifest_text).await?;

    let mut tx = db.pool.begin().await?;
    let project_id: Uuid = sqlx::query_scalar(
        "INSERT INTO projects (workspace_id, path, name, default_branch, vcs, kind)
         VALUES ($1, $2, $3, 'main', 'git', 'app')
         ON CONFLICT (path) DO UPDATE SET name = EXCLUDED.name, kind = 'app'
         RETURNING id",
    )
    .bind(workspace_id)
    .bind(path.to_string_lossy().as_ref())
    .bind(&parsed.name)
    .fetch_one(&mut *tx)
    .await?;

    let scopes: Vec<String> = parsed.scopes.iter().map(|s| s.as_str().to_string()).collect();
    sqlx::query(
        "INSERT INTO apps (id, project_id, workspace_id, slug, name, icon, summary, brief,
                           runtime, state, schema_name, manifest, manifest_sha256,
                           requested_scopes, port)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'active', $10, $11, $12, $13, $14)",
    )
    .bind(id)
    .bind(project_id)
    .bind(workspace_id)
    .bind(&slug)
    .bind(&parsed.name)
    .bind(&parsed.icon)
    .bind(&parsed.summary)
    .bind(brief)
    .bind(parsed.runtime.as_str())
    .bind(&schema)
    .bind(manifest_text)
    .bind(digest(manifest_text))
    .bind(&scopes)
    .bind(parsed.port.map(i32::from))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get(db, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the app vanished immediately after being created"))
}

/// Replace an app's manifest, in the database and in its folder.
///
/// Both, always, and through one function — the same shape the Files tab uses
/// when a person saves. The database copy is what the renderer reads on every
/// page load; the folder copy is what git tracks and what export reads.
pub async fn set_manifest(db: &Db, app: &App, manifest_text: &str) -> anyhow::Result<Manifest> {
    let parsed = manifest::parse(manifest_text)?;
    if parsed.runtime != app.runtime {
        anyhow::bail!(
            "this app is a {} app and the new manifest says {} — changing what runs an \
             app is not an edit, it is a different app",
            app.runtime.as_str(),
            parsed.runtime.as_str()
        );
    }
    write_manifest(&app.path, manifest_text).await?;

    let scopes: Vec<String> = parsed.scopes.iter().map(|s| s.as_str().to_string()).collect();
    sqlx::query(
        "UPDATE apps SET manifest = $2, manifest_sha256 = $3, name = $4, icon = $5,
                         summary = $6, requested_scopes = $7, port = $8, updated_at = now()
          WHERE id = $1",
    )
    .bind(app.id)
    .bind(manifest_text)
    .bind(digest(manifest_text))
    .bind(&parsed.name)
    .bind(&parsed.icon)
    .bind(&parsed.summary)
    .bind(&scopes)
    .bind(parsed.port.map(i32::from))
    .execute(&db.pool)
    .await?;
    Ok(parsed)
}

async fn write_manifest(path: &Path, text: &str) -> anyhow::Result<()> {
    tokio::fs::write(path.join(MANIFEST_FILE), text).await?;
    Ok(())
}

/// Switch an app on or off.
///
/// Off means out of the sidebar and, for a container, stopped. It does **not**
/// mean the data is gone: deactivating has to stay free, or nobody will use the
/// switch and the toggle stops being worth having. Losing rows is what
/// `uninstall` is for, and it asks.
pub async fn set_active(db: &Db, id: Uuid, active: bool) -> anyhow::Result<()> {
    sqlx::query("UPDATE apps SET state = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(if active { "active" } else { "inactive" })
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// Remove an app, its tables, and its folder.
///
/// The only destructive verb, and the only thing that drops a schema. The
/// database goes first: a folder deleted while the rows survive leaves an app
/// nothing can render and nothing can repair, whereas rows outliving a folder
/// by a moment are merely untidy.
pub async fn uninstall(db: &Db, id: Uuid) -> anyhow::Result<()> {
    let Some(app) = get(db, id).await? else {
        return Ok(());
    };
    // Identifier, not a bound parameter — DDL cannot take one. Safe because a
    // schema name is derived from a slug, which is generated here from a
    // validated charset and never from anything a manifest supplies.
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", app.schema))
        .execute(&db.pool)
        .await?;
    // Deleting the project cascades to the app row, its builds and its grants.
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(app.project_id)
        .execute(&db.pool)
        .await?;
    if app.path.starts_with(root()) {
        let _ = tokio::fs::remove_dir_all(&app.path).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_is_a_legal_dns_label_whatever_the_name_was() {
        let id = Uuid::nil();
        assert!(slug("Expenses", &id).starts_with("expenses-"));
        assert!(slug("My  Great   App!", &id).starts_with("my-great-app-"));
        // A name of nothing but punctuation still has to produce a hostname,
        // and a hostname may not start with a dash.
        let odd = slug("!!!", &id);
        assert!(odd.starts_with("app-"), "{odd}");
        for s in [slug("Expenses", &id), slug("!!!", &id), slug("A-B-", &id)] {
            assert!(!s.starts_with('-') && !s.ends_with('-'), "{s}");
            assert!(s.len() <= 63, "{s}");
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{s}"
            );
        }
    }

    #[test]
    fn two_apps_with_one_name_do_not_collide() {
        let name = "Notes";
        let a = slug(name, &Uuid::new_v4());
        let b = slug(name, &Uuid::new_v4());
        assert_ne!(a, b);
    }

    #[test]
    fn a_schema_name_needs_no_quoting() {
        // It is interpolated into DROP SCHEMA, so this is the property that
        // matters more than how pretty it looks.
        let s = schema_name("my-app-a1b2c3");
        assert_eq!(s, "app_my_app_a1b2c3");
        assert!(s
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
    }

    #[test]
    fn the_digest_changes_when_the_text_does() {
        assert_eq!(digest("a"), digest("a"));
        assert_ne!(digest("a"), digest("a "));
    }
}
