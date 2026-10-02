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

    /// Note a question as it is asked, so it outlives this process and the
    /// inbox can show it. No-op by default: the broker's tests keep their
    /// state in memory and have nothing to persist.
    async fn record(
        &self,
        _request_id: &str,
        _run_id: Uuid,
        _tool: &str,
        _input: &serde_json::Value,
    ) {
    }

    /// Note how a question ended. The first close wins; a later one (the guard
    /// dropping after a resolve) finds it already closed.
    async fn close(&self, _request_id: &str, _decision: &'static str) {}
}

/// What the inbox keeps of a tool's input: enough to recognise the question,
/// never the whole payload — a file write's content has no business in a
/// table that outlives the run.
pub fn summarize_input(input: &serde_json::Value) -> String {
    const MAX: usize = 300;
    let text = match input {
        serde_json::Value::Object(map) => [
            "command",
            "file_path",
            "path",
            "url",
            "pattern",
            "description",
        ]
        .iter()
        .find_map(|k| {
            map.get(*k)
                .and_then(|v| v.as_str())
                .map(|v| format!("{k}: {v}"))
        })
        .unwrap_or_else(|| {
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            format!("with {}", keys.join(", "))
        }),
        other => other.to_string(),
    };
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > MAX {
        let cut: String = one_line.chars().take(MAX).collect();
        format!("{cut}…")
    } else {
        one_line
    }
}

/// How long the broker should wait, asked fresh each time.
///
/// A trait for the same reason `RunGate` is one — the permission tests run
/// without a database — but also because a cached copy is what broke this.
/// The broker held a boot-time value while `mcp_tool_timeout_ms` re-read the
/// setting per dispatch, so changing "wait for me" split the two and let the
/// CLI's own timeout win the race, which is precisely what `CLI_GRACE_SECS`
/// exists to prevent. Re-reading makes the agreement structural rather than
/// something every future writer of the settings row has to remember.
#[async_trait]
pub trait Window: Send + Sync + 'static {
    async fn wait(&self) -> Option<std::time::Duration>;
}

pub struct DbWindow(pub Db);

#[async_trait]
impl Window for DbWindow {
    async fn wait(&self) -> Option<std::time::Duration> {
        crate::attention::load(&self.0).await.window()
    }
}

/// A fixed window, for tests and for anything that genuinely has no database.
pub struct FixedWindow(pub Option<std::time::Duration>);

#[async_trait]
impl Window for FixedWindow {
    async fn wait(&self) -> Option<std::time::Duration> {
        self.0
    }
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
            let ctx = crate::attention::ctx_for_run(&self.db, run_id, Some(waiting_for)).await;
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

    async fn record(&self, request_id: &str, run_id: Uuid, tool: &str, input: &serde_json::Value) {
        let _ = sqlx::query(
            "INSERT INTO permission_requests (id, run_id, tool, input_summary)
             VALUES ($1, $2, $3, $4) ON CONFLICT (id) DO NOTHING",
        )
        .bind(request_id)
        .bind(run_id)
        .bind(tool)
        .bind(summarize_input(input))
        .execute(&self.db.pool)
        .await;
    }

    async fn close(&self, request_id: &str, decision: &'static str) {
        let _ = sqlx::query(
            "UPDATE permission_requests SET resolved_at = now(), decision = $2
              WHERE id = $1 AND resolved_at IS NULL",
        )
        .bind(request_id)
        .bind(decision)
        .execute(&self.db.pool)
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::summarize_input;
    use serde_json::json;

    #[test]
    fn a_summary_names_the_part_a_person_recognises() {
        assert_eq!(
            summarize_input(&json!({"command": "cargo   test\n-p x"})),
            "command: cargo test -p x"
        );
        assert_eq!(
            summarize_input(&json!({"file_path": "src/a.rs", "content": "secret"})),
            "file_path: src/a.rs"
        );
        assert_eq!(summarize_input(&json!({"b": 1, "a": 2})), "with a, b");
    }

    #[test]
    fn a_long_input_is_cut_not_stored_whole() {
        let long = "x".repeat(2000);
        let s = summarize_input(&json!({ "command": long }));
        assert!(s.chars().count() <= 302, "{}", s.len());
        assert!(s.ends_with('…'));
    }
}
