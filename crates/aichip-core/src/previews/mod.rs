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
pub mod recipe_writer;

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
    /// The image is still on disk, so starting this again is a wake of a few
    /// seconds rather than a rebuild of several minutes. Worth saying: the
    /// button means something different in each case.
    pub can_wake: bool,
    /// The name this answers to. Only the label — the dashboard builds the URL,
    /// because the port to put in it is the one aichip is being *served* on and
    /// the core has no business knowing that. The browser already does.
    pub slug: Option<String>,
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

/// The hostname suffix previews answer on. Duplicated nowhere: the server's
/// proxy imports this, so the name it routes and the name the UI shows cannot
/// drift apart.
pub const PREVIEW_SUFFIX: &str = ".preview.localhost";

/// How long a preview nobody has looked at stays up.
pub const IDLE_MINUTES: i64 = 30;

/// The two numbers that decide whether previews are safe to forget about.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_live: i64,
    /// Zero means never idle-stop, which is a real choice for someone who
    /// leaves one open all day on purpose.
    pub idle_minutes: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_live: MAX_LIVE,
            idle_minutes: IDLE_MINUTES,
        }
    }
}

pub async fn limits(db: &Db) -> Limits {
    let stored: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'preview_limits'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    let d = Limits::default();
    let Some(v) = stored else { return d };
    Limits {
        // Clamped rather than trusted: a zero here would make the Build button
        // permanently refuse, and there is no way back from that in the UI.
        max_live: v.get("max_live").and_then(|x| x.as_i64()).unwrap_or(d.max_live).clamp(1, 20),
        idle_minutes: v
            .get("idle_minutes")
            .and_then(|x| x.as_i64())
            .unwrap_or(d.idle_minutes)
            .clamp(0, 24 * 60),
    }
}

pub async fn set_limits(db: &Db, next: Limits) -> anyhow::Result<Limits> {
    let next = Limits {
        max_live: next.max_live.clamp(1, 20),
        idle_minutes: next.idle_minutes.clamp(0, 24 * 60),
    };
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('preview_limits', $1)
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .bind(serde_json::json!({
        "max_live": next.max_live,
        "idle_minutes": next.idle_minutes,
    }))
    .execute(&db.pool)
    .await?;
    Ok(next)
}

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
                p.port_assumed, p.error, p.container_id, p.image_kept, p.slug,
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
    // Reading the card is the only "someone is looking at this" signal there
    // is — the drawer does not poll a running preview, and the browser tab
    // showing the container never talks to aichip at all. It is a coarse
    // signal, so the idle window is measured in tens of minutes rather than
    // minutes: the cost of being wrong is a rebuild, not lost work.
    if let Some(r) = &row {
        if r.get::<String, _>("status") == "running" {
            let _ = sqlx::query("UPDATE previews SET last_seen_at = now() WHERE id = $1")
                .bind(r.get::<Uuid, _>("id"))
                .execute(&db.pool)
                .await;
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
            // Only meaningful when it is not already up.
            can_wake: r.get::<bool, _>("image_kept") && status != "running",
            slug: r
                .get::<Option<String>, _>("slug")
                .filter(|_| status == "running"),
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
    let own = tokio::fs::read_to_string(source.join("Dockerfile")).await.ok();

    // A branch's own Dockerfile always wins. Only when there is none do we look
    // for an approved recipe — and only an *approved* one: a proposal is
    // agent-written code that nobody has read yet, and `RUN` executes on this
    // machine. See `recipe_writer` for why that gate is not negotiable.
    let recipe_text = match &own {
        Some(_) => None,
        None => approved_recipe(db, task_id).await?,
    };
    let dockerfile = own.clone().or_else(|| recipe_text.clone());
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
    let limits = limits(db).await;
    if live.len() as i64 >= limits.max_live {
        anyhow::bail!(
            "{} previews are already running, which is the limit — each one holds \
             2 GB and 2 CPUs of this machine. Stop one first: {}.",
            live.len(),
            live.join(", ")
        );
    }

    // Wake before rebuild. An idle-stopped preview kept its image, so putting
    // it back is a `docker run` rather than a `docker build` — seconds instead
    // of minutes, and byte-for-byte what was there before rather than a fresh
    // build of a branch that may have moved on in the meantime.
    if let Some(woken) = wake(db, task_id, port.number).await? {
        return Ok(woken);
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

    // Named after the card and keyed to the card, so a rebuild keeps the name.
    // Written as a second statement only because the insert above may have lost
    // its race, in which case there is no row of ours to name.
    let title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&db.pool)
        .await
        .unwrap_or_default();
    sqlx::query("UPDATE previews SET slug = $2 WHERE id = $1")
        .bind(id)
        .bind(recipe::slug(&title, &task_id))
        .execute(&db.pool)
        .await?;

    let pool = db.pool.clone();
    tokio::spawn(async move {
        if let Err(e) = build_and_run(&pool, id, &source, port.number, recipe_text).await {
            fail(&pool, id, &e).await;
        }
    });

    get(db, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("preview vanished immediately after being created"))
}

/// This project's recipe, but only once a person has approved it.
async fn approved_recipe(db: &Db, task_id: Uuid) -> anyhow::Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT r.dockerfile FROM preview_recipes r
           JOIN tasks t ON t.project_id = r.project_id
          WHERE t.id = $1 AND r.status = 'approved'",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?)
}

/// Put an idle preview back, if its image is still there.
///
/// Returns `None` when there is nothing to wake, so the caller falls through to
/// a normal build. Checks Docker for the image rather than trusting the column:
/// `docker image prune` is a thing people run, and aichip is not told.
async fn wake(db: &Db, task_id: Uuid, container_port: u16) -> anyhow::Result<Option<Preview>> {
    let row = sqlx::query(
        "SELECT id, image, container_port FROM previews
          WHERE task_id = $1 AND status = 'idle' AND image_kept
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(&db.pool)
    .await?;
    let Some(row) = row else { return Ok(None) };

    let id: Uuid = row.get("id");
    let tag = row
        .get::<Option<String>, _>("image")
        .unwrap_or_else(|| recipe::image_tag(&id));
    if !docker::image_exists(&tag).await {
        // Someone reclaimed it behind our back. Say so in the row rather than
        // silently rebuilding under a status that claims to be a wake.
        sqlx::query("UPDATE previews SET image_kept = FALSE WHERE id = $1")
            .bind(id)
            .execute(&db.pool)
            .await?;
        return Ok(None);
    }

    // A woken preview gets a new port: the old one may well have been taken by
    // something else in the meantime, and a stale port in the row would send
    // someone to whatever is there now.
    let host_port = free_port()?;
    let port = row
        .get::<Option<i32>, _>("container_port")
        .map(|p| p as u16)
        .unwrap_or(container_port);
    // The old container is gone by definition, but its name may linger if the
    // idle stop lost a race.
    docker::remove(&recipe::container_name(&id)).await;
    let container = docker::run(&tag, &recipe::container_name(&id), host_port, port)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    sqlx::query(
        "UPDATE previews SET status='running', container_id=$2, host_port=$3,
                             started_at=now(), last_seen_at=now(), error=NULL
          WHERE id=$1",
    )
    .bind(id)
    .bind(&container)
    .bind(host_port as i32)
    .execute(&db.pool)
    .await?;

    tracing::info!(preview=%id, "woke an idle preview from its kept image");
    get(db, task_id).await
}

/// Stop previews nobody has looked at for a while, keeping their images.
///
/// The container is what costs memory; the image only costs disk. Keeping the
/// image is what makes this safe to do automatically — the cost of being wrong
/// is a few seconds when you come back, not a rebuild.
pub async fn sweep_idle(db: &Db) -> anyhow::Result<u64> {
    let limits = limits(db).await;
    if limits.idle_minutes == 0 {
        return Ok(0);
    }
    let rows = sqlx::query(
        "UPDATE previews SET status='idle', stopped_at=now(), image_kept=TRUE,
                error='stopped after nobody looked at it for a while'
          WHERE status='running'
            AND COALESCE(last_seen_at, started_at, created_at)
                  < now() - make_interval(mins => $1::int)
      RETURNING id, container_id",
    )
    .bind(limits.idle_minutes as i32)
    .fetch_all(&db.pool)
    .await?;

    for row in &rows {
        let id: Uuid = row.get("id");
        if let Some(c) = row.get::<Option<String>, _>("container_id") {
            docker::remove(&c).await;
        }
        docker::remove(&recipe::container_name(&id)).await;
    }
    if !rows.is_empty() {
        tracing::info!(count = rows.len(), "idle previews stopped; images kept for a fast wake");
    }
    Ok(rows.len() as u64)
}

/// The sweeper loop. One minute is far finer than the idle window, which keeps
/// the check cheap and the stop close to when it was earned.
pub async fn idle_loop(db: Db) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        if let Err(e) = sweep_idle(&db).await {
            tracing::warn!(error=%e, "idle preview sweep failed");
        }
    }
}

/// What previews are costing on disk, and what could be reclaimed.
pub async fn disk(db: &Db) -> anyhow::Result<(u64, i64)> {
    let reclaimable: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM previews WHERE image_kept AND status <> 'running'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap_or(0);
    Ok((docker::image_disk_bytes().await, reclaimable))
}

/// Drop every kept image that nothing is running from.
///
/// Explicit rather than automatic: reclaiming an image turns the next wake back
/// into a full rebuild, and that is a trade the person looking at the disk
/// figure should get to make.
pub async fn reclaim_disk(db: &Db) -> anyhow::Result<u64> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE previews SET image_kept = FALSE
          WHERE image_kept AND status <> 'running' RETURNING id",
    )
    .fetch_all(&db.pool)
    .await?;
    for id in &ids {
        docker::remove_image(&recipe::image_tag(id)).await;
    }
    Ok(ids.len() as u64)
}

async fn build_and_run(
    pool: &sqlx::PgPool,
    id: Uuid,
    source: &Path,
    container_port: u16,
    // An approved recipe, when the branch has no Dockerfile of its own.
    dockerfile: Option<String>,
) -> Result<(), String> {
    let tag = recipe::image_tag(&id);
    let name = recipe::container_name(&id);

    docker::build(source, &tag, dockerfile.as_deref()).await?;

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
                             host_port=$4, started_at=now(), last_seen_at=now(),
                             image_kept=TRUE
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
        "UPDATE previews SET status='stopped', stopped_at=now(), image_kept=FALSE
          WHERE task_id=$1 AND status IN ('building','running','idle')
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
