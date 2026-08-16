//! Attachments a user hands to a run: images, PDFs, and small text files.
//!
//! There is no structured content-block path to the engine — `RunSpec.prompt`
//! is one string that becomes a single argv entry — so attachments reach the
//! model as files on disk named by absolute path. Claude Code's `Read` tool
//! renders images and PDFs visually, so this is a real multimodal path rather
//! than a text-only fallback.
//!
//! The bytes deliberately live outside every git tree; see the 0009 migration
//! for why. `augment_prompt` is where a run's prompt learns about them.

use crate::db::Db;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Attachment {
    pub id: Uuid,
    pub filename: String,
    pub mime: String,
    /// image | pdf | text
    pub kind: String,
    pub disk_path: PathBuf,
}

/// Where attachment bytes live: `~/.aichip/attachments/<id>/<filename>`.
///
/// One directory per attachment, so two files called `screenshot.png` never
/// collide and deleting one is a single `remove_dir_all`.
pub fn default_root() -> PathBuf {
    home().join(".aichip").join("attachments")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub async fn for_task(db: &Db, task_id: Uuid) -> anyhow::Result<Vec<Attachment>> {
    load(db, "task_id", task_id).await
}

pub async fn for_message(db: &Db, message_id: Uuid) -> anyhow::Result<Vec<Attachment>> {
    load(db, "message_id", message_id).await
}

/// `column` is a compile-time-known literal from the two callers above, never
/// user input.
async fn load(db: &Db, column: &'static str, owner: Uuid) -> anyhow::Result<Vec<Attachment>> {
    let sql = match column {
        "task_id" => {
            "SELECT id, filename, mime, kind, disk_path FROM attachments
             WHERE task_id = $1 ORDER BY created_at"
        }
        _ => {
            "SELECT id, filename, mime, kind, disk_path FROM attachments
             WHERE message_id = $1 ORDER BY created_at"
        }
    };
    let rows = sqlx::query_as::<_, (Uuid, String, String, String, String)>(sql)
        .bind(owner)
        .fetch_all(&db.pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, filename, mime, kind, disk_path)| Attachment {
            id,
            filename,
            mime,
            kind,
            disk_path: PathBuf::from(disk_path),
        })
        .collect())
}

/// Append an instruction block naming each attachment by absolute path, and
/// return the directories the engine should be granted (`--add-dir`).
///
/// Returns the prompt byte-identical when there is nothing to attach — that is
/// what keeps this feature invisible for the runs that don't use it. Files that
/// have vanished from disk are skipped rather than promised to the model.
///
/// The block goes *after* the user's text so the prompt still opens the way
/// they wrote it, and so the actionable instruction is the last thing read.
pub fn augment_prompt(prompt: &str, atts: &[Attachment]) -> (String, Vec<PathBuf>) {
    let present: Vec<&Attachment> = atts.iter().filter(|a| a.disk_path.is_file()).collect();
    if present.is_empty() {
        return (prompt.to_string(), vec![]);
    }

    let mut block = String::from("\n\n---\n");
    let plural = if present.len() == 1 { "file" } else { "files" };
    block.push_str(&format!(
        "The user attached {} {plural} to this message. Read each one with the \
         Read tool, using the absolute path exactly as written below, before you \
         answer. Images and PDFs are rendered visually by Read. These files live \
         outside the repository — do not copy them into it and do not commit them.\n\n",
        present.len()
    ));
    for (i, a) in present.iter().enumerate() {
        block.push_str(&format!(
            "{}. {} ({}) — {}\n",
            i + 1,
            a.filename,
            a.mime,
            a.disk_path.display()
        ));
    }

    let mut dirs: Vec<PathBuf> = vec![];
    for a in &present {
        if let Some(parent) = a.disk_path.parent() {
            let parent = parent.to_path_buf();
            if !dirs.contains(&parent) {
                dirs.push(parent);
            }
        }
    }

    (format!("{prompt}{block}"), dirs)
}

/// How long an unclaimed upload is kept. Generous on purpose: a composer can
/// legitimately sit open for a long time, and the cost of waiting is disk.
pub const UNCLAIMED_RETENTION_HOURS: i64 = 24;

/// `sweep` with the standard root and retention — what the server runs on a
/// timer, so callers don't have to know the policy.
pub async fn sweep_abandoned(db: &Db) -> anyhow::Result<usize> {
    sweep(
        db,
        &default_root(),
        chrono::Duration::hours(UNCLAIMED_RETENTION_HOURS),
    )
    .await
}

/// Delete unclaimed uploads older than `max_age`, then reap on-disk directories
/// with no surviving row. The second pass is what reclaims bytes after a
/// cascade delete — Postgres can drop the row but not the file.
///
/// Returns the number of directories removed.
pub async fn sweep(db: &Db, root: &Path, max_age: chrono::Duration) -> anyhow::Result<usize> {
    let mut removed = 0usize;

    let abandoned: Vec<(String,)> = sqlx::query_as(
        "DELETE FROM attachments
         WHERE task_id IS NULL AND message_id IS NULL AND created_at < now() - $1::interval
         RETURNING disk_path",
    )
    .bind(max_age)
    .fetch_all(&db.pool)
    .await?;
    for (path,) in abandoned {
        if let Some(dir) = PathBuf::from(path).parent() {
            if tokio::fs::remove_dir_all(dir).await.is_ok() {
                removed += 1;
            }
        }
    }

    // Directory names are attachment ids; anything not in the table is dead.
    let mut on_disk: Vec<(Uuid, PathBuf)> = vec![];
    if let Ok(mut entries) = tokio::fs::read_dir(root).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            if let Ok(id) = Uuid::parse_str(&name.to_string_lossy()) {
                on_disk.push((id, entry.path()));
            }
        }
    }
    if !on_disk.is_empty() {
        let ids: Vec<Uuid> = on_disk.iter().map(|(id, _)| *id).collect();
        let live: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM attachments WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(&db.pool)
            .await?;
        for (id, path) in on_disk {
            if !live.iter().any(|(l,)| *l == id) && tokio::fs::remove_dir_all(&path).await.is_ok() {
                removed += 1;
            }
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::{augment_prompt, Attachment};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn att(dir: &PathBuf, filename: &str, mime: &str, kind: &str) -> Attachment {
        Attachment {
            id: Uuid::new_v4(),
            filename: filename.to_string(),
            mime: mime.to_string(),
            kind: kind.to_string(),
            disk_path: dir.join(filename),
        }
    }

    #[test]
    fn no_attachments_leaves_the_prompt_byte_identical() {
        let (out, dirs) = augment_prompt("fix the flaky test", &[]);
        assert_eq!(out, "fix the flaky test");
        assert!(dirs.is_empty());
    }

    #[test]
    fn attachments_are_listed_by_absolute_path_after_the_user_text() {
        let root = std::env::temp_dir().join(format!("aichip-att-{}", Uuid::new_v4()));
        let a_dir = root.join("a");
        let b_dir = root.join("b");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();
        std::fs::write(a_dir.join("diagram.png"), b"x").unwrap();
        std::fs::write(b_dir.join("spec.pdf"), b"x").unwrap();

        let atts = vec![
            att(&a_dir, "diagram.png", "image/png", "image"),
            att(&b_dir, "spec.pdf", "application/pdf", "pdf"),
        ];
        let (out, dirs) = augment_prompt("look at these", &atts);

        assert!(out.starts_with("look at these"), "user text must lead");
        assert!(out.contains("attached 2 files"));
        assert!(out.contains(&a_dir.join("diagram.png").display().to_string()));
        assert!(out.contains(&b_dir.join("spec.pdf").display().to_string()));
        assert_eq!(dirs, vec![a_dir.clone(), b_dir.clone()]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn two_attachments_in_one_dir_yield_one_add_dir() {
        let root = std::env::temp_dir().join(format!("aichip-att-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("one.txt"), b"x").unwrap();
        std::fs::write(root.join("two.txt"), b"x").unwrap();

        let atts = vec![
            att(&root, "one.txt", "text/plain", "text"),
            att(&root, "two.txt", "text/plain", "text"),
        ];
        let (_, dirs) = augment_prompt("hi", &atts);
        assert_eq!(dirs.len(), 1, "duplicate parents must be deduped");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_files_are_skipped_not_promised_to_the_model() {
        let root = std::env::temp_dir().join(format!("aichip-att-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("here.png"), b"x").unwrap();

        let atts = vec![
            att(&root, "here.png", "image/png", "image"),
            att(&root, "gone.png", "image/png", "image"),
        ];
        let (out, dirs) = augment_prompt("hi", &atts);
        assert!(out.contains("here.png"));
        assert!(!out.contains("gone.png"));
        assert!(
            out.contains("attached 1 file "),
            "singular, and count excludes the missing one"
        );
        assert_eq!(dirs.len(), 1);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn every_attachment_missing_is_the_same_as_none() {
        let atts = vec![att(
            &PathBuf::from("/nope/nowhere"),
            "x.png",
            "image/png",
            "image",
        )];
        let (out, dirs) = augment_prompt("just text", &atts);
        assert_eq!(out, "just text");
        assert!(dirs.is_empty());
    }
}
