//! Apps committed to a repository.
//!
//! `.aichip/apps/<name>/aichip.app.yaml` in any project you have added, read on
//! demand — the same shape as `.aichip/workflows/`, deliberately, because it is
//! the same idea and a team should not have to learn two of them. A module
//! being plain YAML is what makes this reviewable in a pull request.
//!
//! Nothing is watched and nothing syncs on its own. Installing something from a
//! repository is a thing a person does, and doing it automatically would mean a
//! `git pull` could add tables.

use super::{install, App, MANIFEST_FILE};
use crate::Db;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// An app found in a repository, and whether it is here already.
#[derive(Debug, Clone)]
pub struct Found {
    /// The folder name under `.aichip/apps/`.
    pub dir: String,
    pub name: String,
    pub summary: String,
    pub manifest: String,
    /// The problem, when the manifest does not parse. Listed anyway: "this one
    /// is broken" is more use than silently showing three of four.
    pub error: Option<String>,
    /// An app of the same name already installed in this workspace.
    pub installed_as: Option<Uuid>,
}

fn apps_dir(project_path: &Path) -> PathBuf {
    project_path.join(".aichip").join("apps")
}

/// Every app a project offers.
pub async fn scan(db: &Db, workspace_id: Uuid, project_path: &Path) -> anyhow::Result<Vec<Found>> {
    let dir = apps_dir(project_path);
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return Ok(Vec::new());
    };

    let existing = super::list(db, Some(workspace_id)).await?;
    let mut out = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let Ok(manifest) = tokio::fs::read_to_string(entry.path().join(MANIFEST_FILE)).await else {
            continue;
        };
        let dir_name = entry.file_name().to_string_lossy().to_string();
        let parsed = super::manifest::parse(&manifest);
        let (name, summary, error) = match &parsed {
            Ok(m) => (m.name.clone(), m.summary.clone(), None),
            Err(e) => (dir_name.clone(), String::new(), Some(e.to_string())),
        };
        // Matched by name rather than by folder: the name is what a person
        // sees, and two folders producing one name is the collision that
        // actually confuses someone.
        let installed_as = existing
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(&name))
            .map(|a: &App| a.id);

        out.push(Found { dir: dir_name, name, summary, manifest, error, installed_as });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// Install one, or update the one already here.
///
/// An app already installed under that name has its manifest replaced rather
/// than being installed twice — which is what makes "sync" mean sync, and
/// routes the change through the same schema gate as any other manifest edit.
/// Its rows are untouched.
pub async fn adopt(
    db: &Db,
    workspace_id: Uuid,
    project_path: &Path,
    dir: &str,
) -> anyhow::Result<App> {
    // A folder name from a request must not be able to leave the apps
    // directory. Rejected outright rather than normalised: there is no
    // legitimate app folder with a separator in it.
    if dir.is_empty() || dir.contains(['/', '\\']) || dir.starts_with('.') {
        anyhow::bail!("\"{dir}\" is not an app folder name");
    }

    let path = apps_dir(project_path).join(dir).join(MANIFEST_FILE);
    let manifest = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| anyhow::anyhow!("there is no {MANIFEST_FILE} in {dir}"))?;
    let parsed = super::manifest::parse(&manifest)?;

    let existing = super::list(db, Some(workspace_id))
        .await?
        .into_iter()
        .find(|a| a.name.eq_ignore_ascii_case(&parsed.name));

    match existing {
        Some(app) => {
            super::set_manifest(db, &app, &manifest).await?;
            super::get(db, app.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("the app disappeared while it was being updated"))
        }
        None => install(db, workspace_id, &manifest, &format!("synced from {dir}")).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_name_cannot_climb_out_of_the_apps_directory() {
        // `adopt` joins this onto a path, so the check is what keeps it inside.
        for bad in ["../../etc", "a/b", "a\\b", "", ".hidden", "./x"] {
            let rejected =
                bad.is_empty() || bad.contains(['/', '\\']) || bad.starts_with('.');
            assert!(rejected, "{bad} would have been accepted");
        }
        for good in ["expenses", "my-app", "app_2"] {
            let rejected =
                good.is_empty() || good.contains(['/', '\\']) || good.starts_with('.');
            assert!(!rejected, "{good} was rejected");
        }
    }

    #[test]
    fn apps_live_where_workflows_do() {
        let p = apps_dir(Path::new("/repo"));
        assert!(p.ends_with("apps"));
        assert!(p.to_string_lossy().contains(".aichip"));
    }
}
