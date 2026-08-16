//! One engine step's token tally, reconciled against the engine's own total.
//!
//! Both adapters report usage twice: once per assistant message as the run
//! goes (`UsageUpdated`), and once authoritatively at the end
//! (`RunCompleted`). Adding both to the same row double-counts every run —
//! silently, and in the direction that makes everything look more expensive
//! than it was. This type exists so that cannot happen: it tracks what has
//! already been written and only ever emits the *difference*.
//!
//! Why bother with the mid-run figures at all, when the final message is
//! authoritative? Because a run that is cancelled, or whose engine dies, never
//! sends that message — and today those runs record zero tokens, which is how
//! a twenty-minute session you interrupted shows up as free.
//!
//! ## What the mid-run numbers mean
//!
//! The two engines disagree, and neither is wrong:
//!
//! - **Claude Code** reports each *request's* usage, so `input_tokens` grows
//!   with the conversation (120 → 140 → 200 → 260 across one run). Its own
//!   final total reports the **last** input and the **summed** output.
//! - **OpenCode** reports each message's own usage, disjoint per `messageID`,
//!   and its adapter **sums** them into the final total. (Established by
//!   `opencode/fixtures/README.md`: step 2 costs *less* than step 1, so a
//!   `step_finish` is not a running total.)
//!
//! So [`UsageTally::observe`] sums outputs (they never overlap) and takes the
//! maximum of the cumulative counters (summing those would multiply Claude's
//! input several-fold). That is exact for Claude and an under-estimate for
//! OpenCode — a floor, not a guess upward.
//!
//! That approximation only ever survives on a run that ended *without* a final
//! message, and such a run is flagged `tokens_provisional`. Every run that
//! finishes normally is reconciled to the engine's own figures exactly — and
//! OpenCode synthesises its final message on EOF rather than receiving one, so
//! in practice the engine the rule is weakest for is also the one least likely
//! to need it.

use aichip_shared::Usage;

/// A signed change to apply to a row's counters.
///
/// Signed because [`UsageTally::reconcile`] may need to *correct downward*: a
/// mid-run estimate can overshoot the engine's final word, and a tally that
/// could only add would leave the overshoot on the row forever.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageDelta {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

impl UsageDelta {
    /// Nothing changed, so there is nothing worth a database round trip.
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Default)]
pub struct UsageTally {
    /// What has already been written to the row.
    flushed: Usage,
    /// The best current estimate of this step's total.
    live: Usage,
    /// Whether the engine's own final figures have replaced the estimate.
    reconciled: bool,
}

impl UsageTally {
    /// Fold in one mid-run report. See the module docs for why outputs sum and
    /// the other three take a maximum.
    pub fn observe(&mut self, u: &Usage) {
        self.live.output_tokens += u.output_tokens;
        self.live.input_tokens = self.live.input_tokens.max(u.input_tokens);
        self.live.cache_read_tokens = self.live.cache_read_tokens.max(u.cache_read_tokens);
        self.live.cache_creation_tokens =
            self.live.cache_creation_tokens.max(u.cache_creation_tokens);
    }

    /// The change since the last flush, marking it as written.
    ///
    /// Callers must actually persist the result: it is subtracted from the
    /// running total either way, so a dropped delta is lost.
    pub fn take_delta(&mut self) -> UsageDelta {
        let d = UsageDelta {
            input: diff(self.live.input_tokens, self.flushed.input_tokens),
            output: diff(self.live.output_tokens, self.flushed.output_tokens),
            cache_read: diff(self.live.cache_read_tokens, self.flushed.cache_read_tokens),
            cache_creation: diff(
                self.live.cache_creation_tokens,
                self.flushed.cache_creation_tokens,
            ),
        };
        self.flushed = self.live.clone();
        d
    }

    /// Replace the estimate with the engine's own figures, without writing.
    ///
    /// Separate from [`Self::reconcile`] so the streaming loop can adopt the
    /// final numbers as they arrive and still write the step's tokens exactly
    /// once, after the loop — one UPDATE per step rather than one per message.
    pub fn adopt(&mut self, authoritative: &Usage) {
        self.live = authoritative.clone();
        self.reconciled = true;
    }

    /// Replace the estimate with the engine's own figures.
    ///
    /// The returned delta moves the row from whatever was flushed to exactly
    /// what the engine reported — without needing to know what other steps of
    /// the same run wrote, which is what keeps a parallel fan-out correct.
    pub fn reconcile(&mut self, authoritative: &Usage) -> UsageDelta {
        self.adopt(authoritative);
        self.take_delta()
    }

    /// Did this step end on an estimate rather than the engine's own total?
    ///
    /// Reads `live`, not `flushed`, so the answer is the same whether it is
    /// asked before or after the final flush — the caller needs it *before*,
    /// to know what to mark the row as.
    pub fn is_provisional(&self) -> bool {
        !self.reconciled && self.live != Usage::default()
    }
}

/// `u64` counters, `i64` deltas — a downward correction is legitimate, and
/// wrapping subtraction here would turn a small overshoot into 18 quintillion.
fn diff(now: u64, before: u64) -> i64 {
    now as i64 - before as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64, read: u64, creation: u64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: read,
            cache_creation_tokens: creation,
        }
    }

    #[test]
    fn a_completed_run_records_the_engines_total_exactly_once() {
        // The double-count this whole type exists to prevent: mid-run reports
        // arrive *and* the final message repeats them. The row must end up at
        // the engine's number, not at the sum of both.
        let mut t = UsageTally::default();
        t.observe(&usage(120, 12, 0, 0));
        t.observe(&usage(260, 18, 0, 0));
        let mid = t.take_delta();
        let end = t.reconcile(&usage(260, 30, 0, 0));

        assert_eq!(mid.input + end.input, 260);
        assert_eq!(mid.output + end.output, 30);
        assert!(!t.is_provisional());
    }

    #[test]
    fn claudes_growing_input_is_not_summed() {
        // 120 + 140 + 200 + 260 would be 720 for a run that used 260.
        let mut t = UsageTally::default();
        for input in [120, 140, 200, 260] {
            t.observe(&usage(input, 10, 0, 0));
        }
        let d = t.take_delta();
        assert_eq!(
            d.input, 260,
            "cumulative input must take the max, not the sum"
        );
        assert_eq!(d.output, 40, "per-message output must sum");
    }

    #[test]
    fn a_run_that_never_finishes_keeps_its_estimate() {
        // The cancelled-run bug: without this, the row stays at zero and a
        // twenty-minute session looks free.
        let mut t = UsageTally::default();
        t.observe(&usage(500, 40, 300, 100));

        // Asked *before* the flush, which is when the caller needs it — it has
        // to know whether to mark the row as an estimate as it writes.
        assert!(
            t.is_provisional(),
            "no final message means the figures are an estimate"
        );
        let d = t.take_delta();
        assert_eq!(
            (d.input, d.output, d.cache_read, d.cache_creation),
            (500, 40, 300, 100)
        );
        assert!(
            t.is_provisional(),
            "and the answer must not change once written"
        );
    }

    #[test]
    fn reconcile_can_correct_downward() {
        // An estimate that overshot must be walked back, not left on the row.
        let mut t = UsageTally::default();
        t.observe(&usage(900, 90, 0, 0));
        let mid = t.take_delta();
        let end = t.reconcile(&usage(400, 40, 0, 0));

        assert_eq!(end.input, -500);
        assert_eq!(mid.input + end.input, 400);
    }

    #[test]
    fn cache_counters_survive_reconciliation() {
        // The whole point of the feature: these are the numbers that say a run
        // was cheap. Dropping them here would be the original bug, moved.
        let mut t = UsageTally::default();
        let d = t.reconcile(&usage(100, 10, 4_000, 250));
        assert_eq!(d.cache_read, 4_000);
        assert_eq!(d.cache_creation, 250);
    }

    #[test]
    fn nothing_observed_is_not_provisional_and_needs_no_write() {
        // A run that produced no usage at all must not be flagged as an
        // estimate, or every trivial run would carry the warning.
        let mut t = UsageTally::default();
        assert!(t.take_delta().is_zero());
        assert!(!t.is_provisional());
    }

    #[test]
    fn a_flushed_tally_repeats_nothing() {
        let mut t = UsageTally::default();
        t.observe(&usage(100, 10, 0, 0));
        assert!(!t.take_delta().is_zero());
        assert!(t.take_delta().is_zero(), "a second flush must be a no-op");
    }
}
