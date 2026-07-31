//! Run a card's branch and look at it.
//!
//! A card in review is a diff, and reading a diff is a poor way to answer "does
//! this look right". This builds the branch's own Dockerfile and publishes one
//! container on one loopback port, so the question can be answered by looking.
//!
//! Deliberately *not* a deployment feature. It manages no standing
//! environments and assigns no hostnames; a preview exists because you are
//! looking at a card right now.
//!
//! It does outlive an aichip restart, but only as far as the container itself
//! does — `reconcile` keeps the row and the container agreeing rather than
//! killing one to match the other. The container carries `--restart no`, so a
//! Docker restart ends it and the next reconcile records that.
//!
//! ## What keeps this from eating the machine
//!
//! The machine running these previews is also running the user's editor, their
//! other projects, and aichip itself. So: one container per card enforced by a
//! partial unique index, hard memory/CPU/pid caps (`docker::run`), loopback-only
//! publishing, and reconciliation at boot that reads *Docker* rather than the
//! database. Nothing here creates a network or a named volume, which is what
//! keeps a preview from colliding with the compose stacks the user already runs.

pub mod docker;
pub mod recipe;

use crate::db::Db;
use sqlx::Row;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// A preview as the API reports it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: Uuid,
    pub task_id: Uuid,
    pub status: String,
    pub url: Option<String>,
    pub host_port: Option<i32>,
    pub container_port: Option<i32>,
    /// True when nothing in the Dockerfile said which port to publish, so the
    /// number above is a guess. Surfaced because the symptom of a wrong guess
    /// is a blank page, which looks like a bug in the branch.
    pub port_assumed: bool,
    /// The card has been worked on since this was built, so it is serving
    /// history. Reported rather than acted on: killing what someone is looking
    /// at is worse than telling them it is old, and "what did it look like
    /// before?" is a real question.
    pub stale: bool,
    pub error: Option<String>,
}

/// How many previews may run at once.
///
/// Each one is capped at 2 GB and 2 CPUs, so this is the number that decides
/// whether a laptop keeps working while you review a column of cards. Three is
/// deliberately low — the failure it prevents is not "a preview is slow", it is
/// "the machine running your editor stops responding". Slice 4 makes it a
/// setting; until then a wrong constant is much cheaper than no constant.
pub const MAX_LIVE: i64 = 3;

/// Where this task's work lives on disk.
///
/// The most recent run that produced a worktree, falling back to the project's
/// own checkout for a project with no version control — those tasks edit in
/// place, so the checkout *is* the branch.
async fn source_path(db: &Db, task_id: Uuid) -> anyhow::Result<Option<PathBuf>> {
    let row = sqlx::query(
        "SELECT COALESCE(
                    (SELECT r.worktree_path FROM runs r
                      WHERE r.task_id = t.id AND r.worktree_path IS NOT NULL
                      ORDER BY r.created_at DESC LIMIT 1),
                    p.path
                ) AS path
           FROM tasks t JOIN projects p ON p.id = t.project_id
          WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row
        .and_then(|r| r.get::<Option<String>, _>("path"))
        .map(PathBuf::from))
}

/// A free loopback port, asked of the OS rather than picked from a range.
///
/// There is a gap between letting go of this and Docker binding it, so this is
/// a hint and not a reservation — Docker fails loudly if it loses the race, and
/// the row records that instead of a preview that silently isn't there.
fn free_port() -> anyhow::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// The current preview for a task, alive or not.
pub async fn get(db: &Db, task_id: Uuid) -> anyhow::Result<Option<Preview>> {
    let row = sqlx::query(
        "SELECT p.id, p.task_id, p.status, p.host_port, p.container_port,
                p.port_assumed, p.error, p.container_id,
                -- Any run that started after this was built means the branch
                -- has moved on. Cheaper than a column, and it cannot fall out
                -- of step with the runs it is describing.
                EXISTS (SELECT 1 FROM runs r
                         WHERE r.task_id = p.task_id AND r.created_at > p.created_at)
                    AS stale
           FROM previews p WHERE p.task_id = $1
          ORDER BY p.created_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?;

    // A container can die on its own — out of memory, a crash minutes in — and
    // nothing would notice until the next restart, leaving the card offering a
    // link to a closed port. Checked here rather than on a timer because this
    // is read when someone opens the card, which is exactly when it matters.
    if let Some(r) = &row {
        if r.get::<String, _>("status") == "running" {
            let container: Option<String> = r.get("container_id");
            let name = container.unwrap_or_else(|| recipe::container_name(&r.get("id")));
            if !docker::is_running(&name).await {
                let id: Uuid = r.get("id");
                sqlx::query(
                    "UPDATE previews SET status='stopped', stopped_at=now(),
                            error=COALESCE(error, 'its container stopped on its own')
                      WHERE id=$1 AND status='running'",
                )
                .bind(id)
                .execute(&db.pool)
                .await?;
                docker::remove_image(&recipe::image_tag(&id)).await;
                return Box::pin(get(db, task_id)).await;
            }
        }
    }

    Ok(row.map(|r| {
        let host_port = r.get::<Option<i32>, _>("host_port");
        let status = r.get::<String, _>("status");
        Preview {
            id: r.get("id"),
            task_id: r.get("task_id"),
            // A URL only while there is something on the other end of it.
            url: host_port
                .filter(|_| status == "running")
                .map(|p| format!("http://127.0.0.1:{p}")),
            host_port,
            container_port: r.get("container_port"),
            port_assumed: r.get("port_assumed"),
            stale: r.get("stale"),
            error: r.get("error"),
            status,
        }
    }))
}

/// Begin a preview: claim the row, then build and run in the background.
///
/// Returns as soon as the row exists, because a build takes minutes and the
/// button should not hang for them. Everything after that is reported through
/// the row's status.
pub async fn start(db: &Db, task_id: Uuid) -> anyhow::Result<Preview> {
    let Some(source) = source_path(db, task_id).await? else {
        anyhow::bail!("no such task");
    };
    if !source.exists() {
        anyhow::bail!(
            "this card's worktree is gone from disk ({}) — re-run the task to recreate it",
            source.display()
        );
    }

    // Read the recipe before claiming the row: a branch with no Dockerfile
    // should say so immediately rather than leaving a failed row behind.
    let dockerfile = tokio::fs::read_to_string(source.join("Dockerfile"))
        .await
        .ok();
    let port = recipe::plan(dockerfile.as_deref()).map_err(|e| anyhow::anyhow!(e.message()))?;

    // Checked before the row is claimed, and named in the message: "too many
    // previews" with no way to see which ones is a dead end, and the cards
    // holding the slots are usually ones the person has forgotten about.
    let live: Vec<String> = sqlx::query_scalar(
        "SELECT t.title FROM previews p JOIN tasks t ON t.id = p.task_id
          WHERE p.status IN ('building','running') AND p.task_id <> $1
          ORDER BY p.created_at",
    )
    .bind(task_id)
    .fetch_all(&db.pool)
    .await?;
    if live.len() as i64 >= MAX_LIVE {
        anyhow::bail!(
            "{} previews are already running, which is the limit — each one holds \
             2 GB and 2 CPUs of this machine. Stop one first: {}.",
            live.len(),
            live.join(", ")
        );
    }

    let project_id: Uuid = sqlx::query_scalar("SELECT project_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&db.pool)
        .await?;

    // The partial unique index is what makes a double-click safe: the second
    // insert loses rather than starting a second container nothing points at.
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO previews
             (task_id, project_id, status, container_port, port_assumed, source_path)
         VALUES ($1, $2, 'building', $3, $4, $5)
         ON CONFLICT DO NOTHING
         RETURNING id",
    )
    .bind(task_id)
    .bind(project_id)
    .bind(port.number as i32)
    .bind(port.source == recipe::PortSource::Assumed)
    .bind(source.to_string_lossy().to_string())
    .fetch_optional(&db.pool)
    .await?;

    let Some(id) = id else {
        // Already building or running — hand back what is there rather than
        // reporting an error for what is arguably success.
        return get(db, task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("a preview is already starting for this card"));
    };

    let pool = db.pool.clone();
    tokio::spawn(async move {
        if let Err(e) = build_and_run(&pool, id, &source, port.number).await {
            fail(&pool, id, &e).await;
        }
    });

    get(db, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("preview vanished immediately after being created"))
}

async fn build_and_run(
    pool: &sqlx::PgPool,
    id: Uuid,
    source: &Path,
    container_port: u16,
) -> Result<(), String> {
    let tag = recipe::image_tag(&id);
    let name = recipe::container_name(&id);

    docker::build(source, &tag).await?;

    // Asked for after the build, not before: a build takes minutes, and a port
    // reserved at the start of one is a port held against the rest of the
    // machine for no reason.
    let host_port = free_port().map_err(|e| format!("no free port: {e}"))?;
    let container = docker::run(&tag, &name, host_port, container_port).await?;

    // A container that starts and exits immediately — a bad CMD, a missing
    // env var — reports success from `docker run` and then is not there. Give
    // it a moment and check, so "running" means running.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    if !docker::is_running(&container).await {
        let why = docker::logs(&container, 20).await;
        docker::remove(&container).await;
        docker::remove_image(&tag).await;
        return Err(if why.is_empty() {
            "the container exited immediately and logged nothing".into()
        } else {
            format!("the container exited immediately:\n{why}")
        });
    }

    sqlx::query(
        "UPDATE previews SET status='running', container_id=$2, image=$3,
                             host_port=$4, started_at=now()
          WHERE id=$1 AND status='building'",
    )
    .bind(id)
    .bind(&container)
    .bind(&tag)
    .bind(host_port as i32)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn fail(pool: &sqlx::PgPool, id: Uuid, why: &str) {
    if let Err(e) = sqlx::query(
        "UPDATE previews SET status='failed', error=$2, stopped_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(why)
    .execute(pool)
    .await
    {
        // Losing this write means a row stuck at "building" forever, which the
        // boot sweep will clear — but it is worth knowing it happened.
        tracing::warn!(preview=%id, error=%e, "could not record a preview failure");
    }
}

/// Stop a task's preview and let go of its image.
///
/// Removal is best effort against Docker but authoritative in the database: a
/// container the user already killed by hand must still leave the row stopped,
/// or the card offers Stop forever and never Preview.
pub async fn stop(db: &Db, task_id: Uuid) -> anyhow::Result<bool> {
    let row = sqlx::query(
        "UPDATE previews SET status='stopped', stopped_at=now()
          WHERE task_id=$1 AND status IN ('building','running')
      RETURNING id, container_id, image",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?;
    let Some(row) = row else { return Ok(false) };

    // By name as well as by id: a row that failed between `docker run` and the
    // status write has no container id, but the container is named after the
    // row and is very much there.
    let id: Uuid = row.get("id");
    if let Some(container) = row.get::<Option<String>, _>("container_id") {
        docker::remove(&container).await;
    }
    docker::remove(&recipe::container_name(&id)).await;
    if let Some(image) = row.get::<Option<String>, _>("image") {
        docker::remove_image(&image).await;
    } else {
        docker::remove_image(&recipe::image_tag(&id)).await;
    }
    Ok(true)
}

/// Settle previews against what Docker actually has. Called once at boot.
///
/// Both directions matter, and only one of them is obvious:
///
/// * a row says running but the container is gone — mark it stopped, or the
///   card offers a dead link;
/// * a container is up but no row claims it — remove it, or every restart
///   leaves another orphan holding a port, and nothing in the UI will ever
///   mention it again.
///
/// The second is why this reads Docker rather than iterating the table.
pub async fn reconcile(db: &Db) -> anyhow::Result<(u64, usize)> {
    if docker::detect().await.is_none() {
        return Ok((0, 0));
    }
    let alive = docker::list_owned().await;

    let rows = sqlx::query(
        "SELECT id, container_id FROM previews WHERE status IN ('building','running')",
    )
    .fetch_all(&db.pool)
    .await?;

    let mut claimed = std::collections::HashSet::new();
    let mut dead = Vec::new();
    for row in &rows {
        let id: Uuid = row.get("id");
        let name = recipe::container_name(&id);
        if alive.contains(&name) && docker::is_running(&name).await {
            claimed.insert(name);
        } else {
            dead.push(id);
        }
    }

    // Reclaim each dead row's image as well as its row. Settling the status
    // alone leaks a few hundred megabytes per preview, silently and forever —
    // the container is gone, so nothing else will ever mention the image
    // again. Derived from the id rather than read from the row because a
    // preview that died mid-build never recorded one.
    for id in &dead {
        docker::remove(&recipe::container_name(id)).await;
        docker::remove_image(&recipe::image_tag(id)).await;
    }

    let settled = if dead.is_empty() {
        0
    } else {
        sqlx::query(
            "UPDATE previews SET status='stopped', stopped_at=now(),
                    error=COALESCE(error, 'its container is no longer running')
              WHERE id = ANY($1)",
        )
        .bind(&dead)
        .execute(&db.pool)
        .await?
        .rows_affected()
    };

    // Anything wearing our label that no live row claims. The label is what
    // makes this safe — a container the user started themselves cannot match.
    let mut swept = 0usize;
    for name in alive {
        if !claimed.contains(&name) {
            docker::remove(&name).await;
            // The image is named from the same id the container is, so an
            // orphan with no row at all can still have its image reclaimed.
            if let Some(short) = name.strip_prefix("aichip-preview-") {
                docker::remove_image(&format!("aichip-preview:{short}")).await;
            }
            swept += 1;
        }
    }
    if settled > 0 || swept > 0 {
        tracing::info!(settled, swept, "previews reconciled with docker at boot");
    }
    Ok((settled, swept))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_port_is_actually_bindable() {
        let port = free_port().unwrap();
        assert!(port > 0);
        // Released, not held: the whole point is that Docker can take it next.
        std::net::TcpListener::bind(("127.0.0.1", port)).expect("port was not released");
    }
}
