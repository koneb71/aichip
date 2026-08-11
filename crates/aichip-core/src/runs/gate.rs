//! What the permission broker needs from the rest of the world.
//!
//! Behind a trait because this repository has no database-backed tests — every
//! test here is either pure or drives real git in a tempdir. The broker's state
//! machine (a refcount, a borrowed queue slot, a timeout that must not be
//! mistaken for a refusal) is exactly the kind of thing that deserves to be
//! asserted directly rather than inferred from an integration run, which is the
//! same reason `resolve_step_permission` was made pure.

use async_trait::async_trait;
use uuid::Uuid;

use crate::db::Db;

#[async_trait]
pub trait RunGate: Send + Sync + 'static {
    /// `running` → `waiting_permission`, recording what is being waited on.
    ///
    /// **False means the run is in no state to wait** — cancelled, failed, or
    /// already finished between the engine deciding to ask and the request
    /// arriving. The guarded UPDATE's own `rows_affected` answers that, so
    /// there is no second query and no window between asking and acting.
    async fn park(&self, run_id: Uuid, waiting_for: &str) -> bool;

    /// `waiting_permission` → `running`, clearing the note.
    async fn unpark(&self, run_id: Uuid);

    /// Nobody answered. Record why, then stop the run.
    async fn abandon(&self, run_id: Uuid, reason: String);
}

pub struct DbGate {
    db: Db,
    cancel: Box<dyn Fn(Uuid) + Send + Sync>,
}

impl DbGate {
    /// `cancel` is a closure rather than an `Arc<Orchestrator>` to keep this
    /// out of the orchestrator's reference cycle: the orchestrator owns the
    /// broker's slots, the broker owns this, and an `Arc` back would keep both
    /// alive forever.
    pub fn new(db: Db, cancel: impl Fn(Uuid) + Send + Sync + 'static) -> Self {
        Self {
            db,
            cancel: Box::new(cancel),
        }
    }
}

#[async_trait]
impl RunGate for DbGate {
    async fn park(&self, run_id: Uuid, waiting_for: &str) -> bool {
        // Guarded the way `finish` guards its card move: a no-op rather than a
        // clobber. It cannot resurrect a cancelled run, and it cannot strand a
        // finished one, because `finish` and `recover_orphans` both already
        // list `waiting_permission` among the statuses they settle.
        let parked = sqlx::query(
            "UPDATE runs SET status='waiting_permission', error_reason=$2
             WHERE id=$1 AND status IN ('starting','running')",
        )
        .bind(run_id)
        .bind(format!("waiting for you to allow {waiting_for}"))
        .execute(&self.db.pool)
        .await
        .map(|r| r.rows_affected() > 0)
        .unwrap_or(false);

        // Fired here rather than at every prompt, because `park` is called
        // exactly once per run — on the first outstanding question. Claude Code
        // issues tool calls in parallel, so the alternative is five shells for
        // one moment of needing you.
        if parked {
            let ctx =
                crate::attention::ctx_for_run(&self.db, run_id, Some(waiting_for)).await;
            crate::attention::fire(&self.db, crate::attention::Event::Permission, ctx).await;
        }
        parked
    }

    async fn unpark(&self, run_id: Uuid) {
        // Clearing `error_reason` is half of a matched pair: `finish` coalesces
        // rather than overwrites, so without this a run that parked once would
        // report "waiting for you to allow Bash" after finishing cleanly.
        let _ = sqlx::query(
            "UPDATE runs SET status='running', error_reason=NULL
             WHERE id=$1 AND status='waiting_permission'",
        )
        .bind(run_id)
        .execute(&self.db.pool)
        .await;
    }

    async fn abandon(&self, run_id: Uuid, reason: String) {
        // The terminal status is set *here*, synchronously, and not left to the
        // cancel signal to produce later. `request` awaits this and only then
        // drops its `ParkGuard`, whose `unpark` would otherwise put the run
        // straight back to `running` — un-cancelling it a moment after it was
        // stopped. That is not hypothetical: it is what happened the first time
        // this ran against a real engine, and the run went on to finish.
        //
        // Setting it first also makes `unpark`'s own guard do the right thing
        // for free, since it only moves a run that is still
        // `waiting_permission`.
        let _ = sqlx::query(
            "UPDATE runs SET status='canceled', error_reason=$2, finished_at=now()
             WHERE id=$1 AND status NOT IN ('completed','failed','canceled')",
        )
        .bind(run_id)
        .bind(reason)
        .execute(&self.db.pool)
        .await;
        let _ = sqlx::query("DELETE FROM queue WHERE run_id=$1")
            .bind(run_id)
            .execute(&self.db.pool)
            .await;
        let _ = sqlx::query(
            "UPDATE steps SET status='skipped', finished_at=now()
             WHERE run_id=$1 AND status IN ('queued','running')",
        )
        .bind(run_id)
        .execute(&self.db.pool)
        .await;
        // And stop the engine, which is still sitting on the tool call.
        (self.cancel)(run_id);
    }
}
