//! The queue's concurrency budget, and the one thing that may temporarily
//! exceed it.
//!
//! `run_loop` holds a permit for the whole of `execute`, which is right while a
//! run is working and wrong while it is waiting for a person. A run parked on a
//! permission prompt is doing nothing, and with the default budget of 2 two of
//! them froze the entire queue for fifteen minutes. The orchestrator already
//! knew — it refuses to start a *scheduled* Reviewed run for exactly this
//! reason — but a manual run still parked, on the assumption that "someone
//! chose to start them and can answer".
//!
//! So a parked run lends its slot back and takes it again on the way out. The
//! borrow is bounded by the number of runs actually waiting on a person, and
//! the invariant that makes it safe — inflate implies owe — lives here rather
//! than at the call sites, so nobody can add a permit without recording the
//! debt.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub struct Slots {
    sem: Arc<Semaphore>,
    /// Slots lent to a parked run that the run has since taken back, and which
    /// are owed to the semaphore.
    ///
    /// Deferred rather than paid on the spot because the alternative is worse
    /// than the bug: shrinking by awaiting `acquire` inside `resolve` would
    /// make the person's **Allow** click block until whatever run took the
    /// borrowed slot finishes. With a twenty-minute run there, the tool call
    /// they just approved times out anyway — so the click would be answered by
    /// the very deadlock it was supposed to break.
    debt: AtomicUsize,
}

impl Slots {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            sem: Arc::new(Semaphore::new(max_concurrent)),
            debt: AtomicUsize::new(0),
        }
    }

    /// The semaphore itself, for the queue loop and the opportunistic
    /// fan-out sites. A slot donated by a parked run is real capacity, and a
    /// fan-out is welcome to it.
    pub fn sem(&self) -> Arc<Semaphore> {
        self.sem.clone()
    }

    /// A run has parked and is not working. Give its slot to the queue.
    pub fn lend(&self) {
        self.sem.add_permits(1);
    }

    /// A parked run is resuming and has taken its slot back. Returns
    /// immediately — see `debt`.
    pub fn reclaim(&self) {
        self.debt.fetch_add(1, Ordering::SeqCst);
    }

    /// Called by the queue loop while holding a permit: true means "this permit
    /// is owed, forget it rather than running something with it".
    ///
    /// A compare-and-swap loop and never `fetch_sub`, which on zero wraps to
    /// `usize::MAX` — every later call would then claim a debt and the queue
    /// would stop dispatching for the life of the process.
    pub fn take_debt(&self) -> bool {
        let mut owed = self.debt.load(Ordering::SeqCst);
        loop {
            if owed == 0 {
                return false;
            }
            match self.debt.compare_exchange_weak(
                owed,
                owed - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return true,
                Err(actual) => owed = actual,
            }
        }
    }

    #[cfg(test)]
    fn debt(&self) -> usize {
        self.debt.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One turn of `run_loop`: take a permit, and if it is owed, forget it and
    /// go round again. Kept in the tests so the assertions below exercise the
    /// same two lines the queue does.
    fn queue_turn(slots: &Slots) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let permit = slots.sem().try_acquire_owned().ok()?;
        if slots.take_debt() {
            permit.forget();
            return None;
        }
        Some(permit)
    }

    #[test]
    fn a_parked_run_frees_a_slot() {
        // The bug, pinned: with the budget spent, nothing else can start.
        let slots = Slots::new(2);
        let _a = slots.sem().try_acquire_owned().unwrap();
        let _b = slots.sem().try_acquire_owned().unwrap();
        assert!(slots.sem().try_acquire_owned().is_err());

        // One of them stops to ask a question, and the queue moves again.
        slots.lend();
        assert!(slots.sem().try_acquire_owned().is_ok());
    }

    #[test]
    fn resuming_never_waits_on_anyone_elses_run() {
        // `reclaim` is the path a person's click is on. It must not block, even
        // with the whole budget spent and nothing about to free up.
        let slots = Slots::new(1);
        // The parked run still holds the permit `run_loop` gave it — parking
        // lends a slot, it does not hand back the one it is standing on.
        let _parked = slots.sem().try_acquire_owned().unwrap();
        slots.lend();
        // Somebody else takes the lent slot and settles in for a long run.
        let _borrowed = slots.sem().try_acquire_owned().unwrap();
        assert!(slots.sem().try_acquire_owned().is_err());

        slots.reclaim();
        assert_eq!(slots.debt(), 1);
    }

    #[test]
    fn the_queue_loop_pays_the_debt_and_capacity_comes_back() {
        let slots = Slots::new(2);
        slots.lend(); // a run parks
        slots.reclaim(); // and resumes

        // The next turn of the loop consumes the extra permit instead of
        // dispatching with it.
        assert!(queue_turn(&slots).is_none());
        assert_eq!(slots.debt(), 0);

        // And the budget is exactly what it was — not one more, not one less.
        let held: Vec<_> = std::iter::repeat_with(|| slots.sem().try_acquire_owned().ok())
            .take_while(|p| p.is_some())
            .collect();
        assert_eq!(held.len(), 2);
    }

    #[test]
    fn a_turn_with_no_debt_dispatches_normally() {
        let slots = Slots::new(1);
        assert!(queue_turn(&slots).is_some());
    }

    #[test]
    fn an_unbalanced_reclaim_never_underflows() {
        // `fetch_sub` here would wrap to usize::MAX and every later turn would
        // forget its permit, stopping the queue for the life of the process.
        let slots = Slots::new(1);
        for _ in 0..1000 {
            assert!(!slots.take_debt());
        }
        assert_eq!(slots.debt(), 0);
        assert!(queue_turn(&slots).is_some());
    }

    #[test]
    fn several_parked_runs_each_lend_one() {
        let slots = Slots::new(1);
        let _working = slots.sem().try_acquire_owned().unwrap();
        slots.lend();
        slots.lend();
        slots.lend();

        let borrowed: Vec<_> = (0..3)
            .map(|_| slots.sem().try_acquire_owned().unwrap())
            .collect();
        assert_eq!(borrowed.len(), 3);
        assert!(slots.sem().try_acquire_owned().is_err());

        // All three answered; three turns of the loop settle it.
        drop(borrowed);
        for _ in 0..3 {
            slots.reclaim();
        }
        for _ in 0..3 {
            assert!(queue_turn(&slots).is_none());
        }
        assert_eq!(slots.debt(), 0);
    }
}
