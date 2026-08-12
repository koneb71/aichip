//! Permission broker: bridges the MCP permission proxy (server side) and the
//! dashboard. A run's engine calls the aichip MCP tool when it needs
//! permission; the broker parks that request, surfaces it as an event, and
//! resolves it when the user clicks Allow/Deny.
//!
//! Two things it is careful about, both learned the hard way.
//!
//! **A parked run is not a working run.** It lends its queue slot back for as
//! long as it waits, because `run_loop` holds a permit across the whole of
//! `execute` and with the default budget of 2 two unanswered questions froze
//! everything. See [`crate::runs::slots`].
//!
//! **Nobody answering is not the same as somebody saying no.** The old
//! behaviour was `_ => false` on timeout, which handed the engine
//! `"denied by aichip user"` — a sentence about a decision no person had made.
//! An engine told it was refused works around the refusal, and spends real
//! money doing it. [`Decision`] keeps the two apart all the way to the wire.

use aichip_shared::{AichipEvent, EventEnvelope};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::bus::EventBus;
use crate::runs::gate::{RunGate, Window};
use crate::runs::slots::Slots;

/// How a permission request ended.
///
/// The distinction that matters is `Denied` versus `Unanswered`: the wire
/// protocol has only allow and deny, so both travel as a denial, but only one
/// of them is true. Collapsing them is what let a run spend an afternoon
/// routing around a decision nobody made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    /// A person clicked Deny.
    Denied,
    /// The window closed with nobody there. Not a refusal.
    Unanswered { waited: Duration },
    /// The run ended while the question was still outstanding.
    RunGone,
}

pub struct PendingPermission {
    pub run_id: Uuid,
    pub tool_name: String,
    pub input: serde_json::Value,
    responder: oneshot::Sender<bool>,
}

/// One run's parked state.
#[derive(Default)]
struct Parked {
    /// How many questions this run has open.
    count: usize,
    /// Whether a slot was actually lent for it.
    ///
    /// Per run rather than per request, and that distinction is the whole
    /// point: only the *first* question lends, but the guard that drops
    /// *last* is the one that has to give it back — and with parallel tool
    /// calls those are routinely different requests. Hanging this off the
    /// guard instead left the run parked forever whenever the first question
    /// happened to be answered first.
    lent: bool,
}

#[derive(Default)]
struct Inner {
    pending: HashMap<String, PendingPermission>,
    /// Outstanding prompts per run. Present means this run is parked: its
    /// status reads `waiting_permission` and it has lent one slot.
    ///
    /// Keyed by run and not by request, because a run holds **one** slot
    /// however many questions it has open — Claude Code issues tool calls in
    /// parallel and several prompts stack on one card. Kept beside `pending`
    /// under the same lock rather than derived by scanning it, because the
    /// refcount is what correctness hinges on and a later refactor of `pending`
    /// must not be able to change it by accident.
    parked: HashMap<Uuid, Parked>,
}

#[derive(Clone)]
pub struct PermissionBroker {
    bus: EventBus,
    inner: Arc<Mutex<Inner>>,
    gate: Arc<dyn RunGate>,
    slots: Arc<Slots>,
    window: Arc<dyn Window>,
}

/// Bookkeeping for one outstanding prompt, undone however the wait ends.
///
/// This exists because `request` is awaited inside an axum handler, and when a
/// run is cancelled the engine's connection closes and hyper drops that future
/// mid-`await`. Anything not in here leaks the first time somebody presses
/// Cancel — today that is already a ghost prompt nothing removes, and with the
/// slot lending it would also be a run stuck reading `waiting_permission` and a
/// permit the semaphore never takes back.
///
/// Exactly one guard exists per request, so the decrement happens exactly once
/// no matter how the wait ended. That is what makes a resolve racing a cancel
/// settle once rather than twice.
struct ParkGuard {
    inner: Arc<Mutex<Inner>>,
    gate: Arc<dyn RunGate>,
    slots: Arc<Slots>,
    run_id: Uuid,
    request_id: String,
}

impl Drop for ParkGuard {
    fn drop(&mut self) {
        let was_lent = {
            let mut inner = self.inner.lock().unwrap();
            inner.pending.remove(&self.request_id);
            match inner.parked.get_mut(&self.run_id) {
                Some(p) if p.count > 1 => {
                    p.count -= 1;
                    None
                }
                Some(_) => inner.parked.remove(&self.run_id).map(|p| p.lent),
                None => None,
            }
        };
        if was_lent == Some(true) {
            self.slots.reclaim();
            let gate = self.gate.clone();
            let run_id = self.run_id;
            tokio::spawn(async move { gate.unpark(run_id).await });
        }
    }
}

impl PermissionBroker {
    /// The window is asked for per request rather than held, so a change in
    /// Settings takes effect on the next prompt instead of the next restart —
    /// and, more importantly, cannot drift from the engine's own timeout, which
    /// is derived from the same setting on every dispatch. A `None` window
    /// means wait indefinitely, which is nearly free now that a parked run
    /// lends its queue slot back.
    pub fn new(
        bus: EventBus,
        gate: Arc<dyn RunGate>,
        slots: Arc<Slots>,
        window: Arc<dyn Window>,
    ) -> Self {
        Self {
            bus,
            inner: Default::default(),
            gate,
            slots,
            window,
        }
    }

    /// Called by the MCP proxy. Blocks until the user resolves, the window
    /// closes, or the run ends.
    pub async fn request(
        &self,
        run_id: Uuid,
        tool_name: String,
        input: serde_json::Value,
    ) -> Decision {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        let first = {
            let mut inner = self.inner.lock().unwrap();
            inner.pending.insert(
                request_id.clone(),
                PendingPermission {
                    run_id,
                    tool_name: tool_name.clone(),
                    input: input.clone(),
                    responder: tx,
                },
            );
            let p = inner.parked.entry(run_id).or_default();
            p.count += 1;
            p.count == 1
        };

        // Constructed before the first `.await` below, so every path out of
        // this function — including the one where the future is dropped —
        // unwinds through it.
        let guard = ParkGuard {
            inner: self.inner.clone(),
            gate: self.gate.clone(),
            slots: self.slots.clone(),
            run_id,
            request_id: request_id.clone(),
        };

        if first {
            // Lend only *after* the guarded UPDATE says the run is alive, so a
            // question from a run that has already been cancelled never
            // inflates the queue's budget.
            if !self.gate.park(run_id, &tool_name).await {
                return Decision::RunGone;
            }
            if let Some(p) = self.inner.lock().unwrap().parked.get_mut(&run_id) {
                p.lent = true;
            }
            self.slots.lend();
        }

        self.bus.publish(EventEnvelope {
            run_id,
            step_id: None,
            seq: -1, // permission events are ephemeral; not part of the replay log
            ts: Utc::now(),
            event: AichipEvent::PermissionRequested {
                request_id: request_id.clone(),
                tool_name: tool_name.clone(),
                input,
            },
        });

        let started = std::time::Instant::now();
        let answer = match self.window.wait().await {
            Some(limit) => tokio::time::timeout(limit, rx).await.map_err(|_| ()),
            None => Ok(rx.await),
        };

        let decision = match answer {
            Ok(Ok(true)) => Decision::Allowed,
            Ok(Ok(false)) => Decision::Denied,
            // The sender was dropped, which only happens when the entry was
            // removed without answering — the run went away underneath us.
            Ok(Err(_)) => Decision::RunGone,
            Err(()) => Decision::Unanswered {
                waited: started.elapsed(),
            },
        };

        if let Decision::Unanswered { waited } = &decision {
            self.gate
                .abandon(
                    run_id,
                    format!(
                        "nobody answered the request to allow {tool_name} after {}; \
                         aichip stopped the run rather than telling it you had refused",
                        humanize(*waited)
                    ),
                )
                .await;
        }

        drop(guard);
        self.bus.publish(EventEnvelope {
            run_id,
            step_id: None,
            seq: -1,
            ts: Utc::now(),
            event: AichipEvent::PermissionResolved {
                request_id,
                allowed: decision == Decision::Allowed,
            },
        });
        decision
    }

    /// Called by the dashboard route when the user clicks Allow/Deny.
    pub fn resolve(&self, request_id: &str, allowed: bool) -> bool {
        // Taken out of `pending` here and *also* removed by the guard on the
        // way out; both are `remove`, so whichever runs second finds nothing
        // and the refcount still moves exactly once.
        let responder = self
            .inner
            .lock()
            .unwrap()
            .pending
            .remove(request_id)
            .map(|p| p.responder);
        match responder {
            Some(tx) => {
                let _ = tx.send(allowed);
                true
            }
            None => false,
        }
    }

    pub fn pending_for_run(&self, run_id: Uuid) -> Vec<(String, String, serde_json::Value)> {
        self.inner
            .lock()
            .unwrap()
            .pending
            .iter()
            .filter(|(_, p)| p.run_id == run_id)
            .map(|(id, p)| (id.clone(), p.tool_name.clone(), p.input.clone()))
            .collect()
    }
}

/// "24h", "15m" — for a sentence handed to an engine and shown to a person.
fn humanize(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct RecordingGate {
        parks: AtomicUsize,
        unparks: AtomicUsize,
        abandons: Mutex<Vec<String>>,
        /// Simulates a run that ended before the question arrived.
        park_fails: bool,
    }

    #[async_trait]
    impl RunGate for RecordingGate {
        async fn park(&self, _run_id: Uuid, _waiting_for: &str) -> bool {
            self.parks.fetch_add(1, Ordering::SeqCst);
            !self.park_fails
        }
        async fn unpark(&self, _run_id: Uuid) {
            self.unparks.fetch_add(1, Ordering::SeqCst);
        }
        async fn abandon(&self, _run_id: Uuid, reason: String) {
            self.abandons.lock().unwrap().push(reason);
        }
    }

    fn broker(
        gate: Arc<RecordingGate>,
        slots: Arc<Slots>,
        timeout: Option<Duration>,
    ) -> PermissionBroker {
        PermissionBroker::new(
            EventBus::new(),
            gate,
            slots,
            Arc::new(crate::runs::gate::FixedWindow(timeout)),
        )
    }

    /// `unpark` is spawned, so give the runtime a turn to run it.
    async fn settle() {
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
    }

    #[tokio::test]
    async fn a_prompt_lends_a_slot_and_answering_takes_it_back() {
        let gate = Arc::new(RecordingGate::default());
        let slots = Arc::new(Slots::new(1));
        let b = broker(gate.clone(), slots.clone(), None);
        let run = Uuid::new_v4();

        let _working = slots.sem().try_acquire_owned().unwrap();
        assert!(slots.sem().try_acquire_owned().is_err());

        let task = {
            let b = b.clone();
            tokio::spawn(async move { b.request(run, "Bash".into(), serde_json::json!({})).await })
        };
        settle().await;

        // The queue can move again while the question stands.
        let borrowed = slots.sem().try_acquire_owned();
        assert!(borrowed.is_ok(), "a parked run must give its slot back");

        let id = b.pending_for_run(run)[0].0.clone();
        assert!(b.resolve(&id, true));
        assert_eq!(task.await.unwrap(), Decision::Allowed);
        settle().await;
        assert_eq!(gate.unparks.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn several_prompts_on_one_run_lend_exactly_one_slot() {
        let gate = Arc::new(RecordingGate::default());
        let slots = Arc::new(Slots::new(0));
        let b = broker(gate.clone(), slots.clone(), None);
        let run = Uuid::new_v4();

        let mut tasks = vec![];
        for _ in 0..3 {
            let b = b.clone();
            tasks.push(tokio::spawn(async move {
                b.request(run, "Bash".into(), serde_json::json!({})).await
            }));
            settle().await;
        }

        // One slot, not three: the run is one run however many questions it
        // has. The permit is bound rather than dropped by `.is_ok()`, which
        // would hand it straight back and make the next line pass for free.
        let lent = slots.sem().try_acquire_owned();
        assert!(lent.is_ok(), "the parked run lends a slot");
        assert!(
            slots.sem().try_acquire_owned().is_err(),
            "and lends exactly one, however many questions it has open"
        );
        assert_eq!(gate.parks.load(Ordering::SeqCst), 1);

        let ids: Vec<_> = b.pending_for_run(run).into_iter().map(|p| p.0).collect();
        assert_eq!(ids.len(), 3);
        for id in &ids[..2] {
            b.resolve(id, true);
        }
        settle().await;
        // Still parked while any question stands.
        assert_eq!(gate.unparks.load(Ordering::SeqCst), 0);

        b.resolve(&ids[2], true);
        for t in tasks {
            t.await.unwrap();
        }
        settle().await;
        assert_eq!(gate.unparks.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_last_answer_gives_the_slot_back_even_though_the_first_lent_it() {
        // Only the first question lends, but any of them may be answered last.
        // Hanging "did we lend?" off the guard rather than off the run left a
        // run parked forever whenever the first question was answered first —
        // which, with tool calls arriving in parallel, is most of the time.
        let gate = Arc::new(RecordingGate::default());
        let slots = Arc::new(Slots::new(0));
        let b = broker(gate.clone(), slots.clone(), None);
        let run = Uuid::new_v4();

        let mut tasks = vec![];
        for _ in 0..2 {
            let b = b.clone();
            tasks.push(tokio::spawn(async move {
                b.request(run, "Bash".into(), serde_json::json!({})).await
            }));
            settle().await;
        }
        let ids: Vec<_> = b.pending_for_run(run).into_iter().map(|p| p.0).collect();

        // Answer them in the order that leaves the *second* guard to settle up.
        for id in &ids {
            b.resolve(id, true);
        }
        for t in tasks {
            t.await.unwrap();
        }
        settle().await;

        assert_eq!(gate.unparks.load(Ordering::SeqCst), 1, "the run must unpark");
        assert!(slots.take_debt(), "and the slot must be owed back");
    }

    #[tokio::test]
    async fn a_prompt_whose_engine_died_gives_the_slot_back() {
        // The dropped-future path. `request` is awaited inside an axum handler,
        // and cancelling a run closes the engine's connection, which drops it
        // mid-await. Before ParkGuard this leaked a prompt nothing removed.
        let gate = Arc::new(RecordingGate::default());
        let slots = Arc::new(Slots::new(0));
        let b = broker(gate.clone(), slots.clone(), None);
        let run = Uuid::new_v4();

        let task = {
            let b = b.clone();
            tokio::spawn(async move { b.request(run, "Bash".into(), serde_json::json!({})).await })
        };
        settle().await;
        assert_eq!(b.pending_for_run(run).len(), 1);

        task.abort();
        settle().await;

        assert!(b.pending_for_run(run).is_empty(), "no ghost prompt");
        assert_eq!(gate.unparks.load(Ordering::SeqCst), 1);
        // And the borrowed slot is owed back rather than kept forever.
        assert!(slots.take_debt());
    }

    #[tokio::test]
    async fn a_prompt_for_a_run_that_already_ended_never_parks_it() {
        let gate = Arc::new(RecordingGate {
            park_fails: true,
            ..Default::default()
        });
        let slots = Arc::new(Slots::new(0));
        let b = broker(gate.clone(), slots.clone(), None);

        let d = b
            .request(Uuid::new_v4(), "Bash".into(), serde_json::json!({}))
            .await;
        assert_eq!(d, Decision::RunGone);
        settle().await;

        // Nothing lent, so nothing owed, and no unpark for a run never parked.
        assert!(slots.sem().try_acquire_owned().is_err());
        assert!(!slots.take_debt());
        assert_eq!(gate.unparks.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn nobody_answering_is_not_a_denial() {
        let gate = Arc::new(RecordingGate::default());
        let slots = Arc::new(Slots::new(0));
        let b = broker(
            gate.clone(),
            slots.clone(),
            Some(Duration::from_secs(24 * 3600)),
        );
        let run = Uuid::new_v4();

        let task = {
            let b = b.clone();
            tokio::spawn(async move { b.request(run, "Bash".into(), serde_json::json!({})).await })
        };
        settle().await;
        tokio::time::advance(Duration::from_secs(24 * 3600 + 1)).await;

        let d = task.await.unwrap();
        assert!(
            matches!(d, Decision::Unanswered { .. }),
            "a closed window is not a person saying no, got {d:?}"
        );

        // And the run is stopped rather than left to route around it.
        let reasons = gate.abandons.lock().unwrap();
        assert_eq!(reasons.len(), 1);
        assert!(reasons[0].contains("Bash"), "{}", reasons[0]);
        assert!(!reasons[0].contains("refused you"), "{}", reasons[0]);
    }

    /// A window that answers differently the second time, standing in for
    /// someone changing "wait for me" in Settings.
    struct ChangingWindow(Mutex<Vec<Option<Duration>>>);

    #[async_trait]
    impl crate::runs::gate::Window for ChangingWindow {
        async fn wait(&self) -> Option<Duration> {
            let mut q = self.0.lock().unwrap();
            if q.len() > 1 { q.remove(0) } else { q[0] }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn changing_the_setting_takes_effect_on_the_next_prompt() {
        // The regression this pins: the broker used to capture its window at
        // boot while the engine's own MCP_TOOL_TIMEOUT was re-derived from the
        // setting on every dispatch. Lower the setting and the two split, so
        // the CLI's timeout fired first and the engine got a bare error it
        // works around — exactly what CLI_GRACE_SECS exists to prevent.
        let gate = Arc::new(RecordingGate::default());
        let b = PermissionBroker::new(
            EventBus::new(),
            gate.clone(),
            Arc::new(Slots::new(0)),
            Arc::new(ChangingWindow(Mutex::new(vec![
                Some(Duration::from_secs(3600)),
                Some(Duration::from_secs(60)),
            ]))),
        );

        // First prompt: the old, long window. Still waiting after a minute.
        let first = {
            let b = b.clone();
            tokio::spawn(async move {
                b.request(Uuid::new_v4(), "Bash".into(), serde_json::json!({})).await
            })
        };
        settle().await;
        tokio::time::advance(Duration::from_secs(61)).await;
        settle().await;
        assert!(!first.is_finished(), "the first prompt kept the window it started with");

        // Second prompt: the new, short one. Closes at 61s.
        let second = {
            let b = b.clone();
            tokio::spawn(async move {
                b.request(Uuid::new_v4(), "Edit".into(), serde_json::json!({})).await
            })
        };
        settle().await;
        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(
            matches!(second.await.unwrap(), Decision::Unanswered { .. }),
            "the second prompt must use the setting as it is now"
        );
        first.abort();
    }

    #[tokio::test]
    async fn a_denial_is_still_a_denial() {
        let gate = Arc::new(RecordingGate::default());
        let b = broker(gate.clone(), Arc::new(Slots::new(0)), None);
        let run = Uuid::new_v4();

        let task = {
            let b = b.clone();
            tokio::spawn(async move { b.request(run, "Bash".into(), serde_json::json!({})).await })
        };
        settle().await;
        let id = b.pending_for_run(run)[0].0.clone();
        b.resolve(&id, false);

        assert_eq!(task.await.unwrap(), Decision::Denied);
        // Nothing to abandon: a person made a decision.
        assert!(gate.abandons.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn resolving_an_unknown_request_is_not_an_error() {
        let b = broker(Arc::new(RecordingGate::default()), Arc::new(Slots::new(0)), None);
        assert!(!b.resolve("no-such-request", true));
    }

    #[test]
    fn a_wait_reads_as_a_person_would_say_it() {
        assert_eq!(humanize(Duration::from_secs(24 * 3600)), "24h");
        assert_eq!(humanize(Duration::from_secs(900)), "15m");
        assert_eq!(humanize(Duration::from_secs(30)), "30s");
    }
}
