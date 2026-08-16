//! Picking a tier from what is already known about the work.
//!
//! `ModelTier::Medium` is the default and maps to Opus, so every card nobody
//! thought about runs on the dearest ordinary model. This exists so a person
//! can say "decide for me" and get something cheaper on the work that doesn't
//! need it.
//!
//! Three rules govern the whole module:
//!
//! 1. **It is a pure function.** No database, no clock, and emphatically no
//!    model call — spawning an engine to decide which engine to spawn would
//!    cost more than it saved and is exactly the thing this is meant to avoid.
//! 2. **First match wins, in a stated order.** Not a score. A score cannot be
//!    explained to the person whose card it just routed, and explaining it is
//!    a requirement here, not a nicety.
//! 3. **Every branch says why.** The reason travels with the decision as one
//!    value, so what gets shown can never drift from what was applied.
//!
//! The do-no-harm property, pinned by a test: with no signals at all, this
//! returns Medium — today's behaviour exactly. Turning Auto on can only make a
//! card cheaper or leave it alone, unless a named signal fired.

use crate::ModelTier;

/// Which pass of a card this run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Reading the code and writing down what it means to do.
    Plan,
    /// Carrying out a plan a person already approved.
    Work,
    /// No plan-first: one pass that does the whole thing.
    Single,
}

/// What is known about a run before it starts.
#[derive(Debug, Clone, Default)]
pub struct Signals {
    /// Title plus description, in characters.
    pub brief_chars: usize,
    pub attachments: usize,
    /// Knowledge-base pages tagged onto the card.
    pub kb_articles: usize,
    /// A previous run on this card failed.
    pub prior_failed: bool,
    /// The tier the previous run on this card used, if any.
    pub prior_tier: Option<ModelTier>,
    /// Times the work has been re-planned.
    pub replans: i32,
}

impl Signals {
    pub fn phase(self, phase: Phase) -> Decision {
        classify(&self, phase)
    }
}

/// A tier, and the reason it was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub tier: ModelTier,
    /// Stable and machine-readable, so the spend view can group by it and
    /// show that one rule is routing badly.
    pub rule: &'static str,
    /// One sentence a person reads on the card.
    pub because: String,
}

/// A brief this short is a one-liner; anything longer has enough in it that
/// the cheap tier is a gamble rather than a saving.
const SHORT_BRIEF: usize = 240;

/// Below this there is not enough written down to conclude *anything*, so no
/// rule may claim the work is small.
///
/// Without this floor an empty brief satisfies "short and unreferenced" and
/// routes to Easy — treating the total absence of information as evidence of
/// simplicity, which is exactly backwards. An under-specified card is harder
/// than a well-described one, not easier.
const MIN_BRIEF: usize = 16;

/// Reference material this heavy is a briefing, not a chore.
const HEAVILY_REFERENCED: usize = 3;

/// Choose a tier. See the module docs for the rules this obeys.
pub fn classify(s: &Signals, phase: Phase) -> Decision {
    // 1. A retry never goes below what already failed. Rerunning a failure on
    //    a cheaper model is paying twice to lose twice.
    if s.prior_failed {
        let tier = step_up(s.prior_tier.unwrap_or(ModelTier::Medium));
        return Decision {
            tier,
            rule: "retry_escalation",
            because: format!(
                "the previous attempt failed, so this one runs at {}",
                name(tier)
            ),
        };
    }

    // 2. Re-planned work has already shown it is not well understood.
    if s.replans > 0 {
        return Decision {
            tier: ModelTier::Complex,
            rule: "replanned",
            because: "the plan has been revised, so the work is not as clear-cut as it looked"
                .into(),
        };
    }

    match phase {
        // 3. Planning is one call that determines the quality of everything
        //    after it. The same argument the codebase already makes for
        //    defaulting a team's planning effort to high.
        Phase::Plan => Decision {
            tier: ModelTier::Complex,
            rule: "planning_pass",
            because: "planning decides what every later pass does, so it is worth the better model"
                .into(),
        },
        // 4. The judgment already happened and a person approved it. This is
        //    the single biggest honest saving here: carrying out an agreed
        //    plan is not the same job as writing one.
        Phase::Work => {
            let tier = step_down(ModelTier::Complex);
            Decision {
                tier,
                rule: "executing_approved_plan",
                because: "the plan is written and approved, so this pass carries it out".into(),
            }
        }
        Phase::Single => single_pass(s),
    }
}

fn single_pass(s: &Signals) -> Decision {
    let referenced = s.attachments + s.kb_articles;

    // 5. A long brief carrying several attachments or runbooks is a briefing.
    if referenced >= HEAVILY_REFERENCED && s.brief_chars > SHORT_BRIEF {
        return Decision {
            tier: ModelTier::Complex,
            rule: "heavily_briefed",
            because: format!(
                "a long brief with {referenced} attached references is not a mechanical change"
            ),
        };
    }

    // 6. Short, unreferenced, first attempt — and long enough to have actually
    //    said something. Note what is *not* claimed here: shortness alone never
    //    buys Easy, because a short brief can just as easily be
    //    under-specified, which is harder rather than easier. It counts only
    //    alongside "nothing else was attached" and "there is a brief at all".
    if (MIN_BRIEF..=SHORT_BRIEF).contains(&s.brief_chars) && referenced == 0 {
        return Decision {
            tier: ModelTier::Easy,
            rule: "small_and_unreferenced",
            because: "a short brief with nothing attached — the cheap model should manage".into(),
        };
    }

    // 7. Nothing said either way. Today's behaviour, unchanged.
    Decision {
        tier: ModelTier::Medium,
        rule: "no_signal",
        because: "nothing about this says it is unusually easy or hard".into(),
    }
}

fn step_up(t: ModelTier) -> ModelTier {
    match t {
        ModelTier::Easy => ModelTier::Medium,
        ModelTier::Medium | ModelTier::Complex => ModelTier::Complex,
    }
}

fn step_down(t: ModelTier) -> ModelTier {
    match t {
        ModelTier::Complex => ModelTier::Medium,
        ModelTier::Medium | ModelTier::Easy => ModelTier::Easy,
    }
}

fn name(t: ModelTier) -> &'static str {
    match t {
        ModelTier::Easy => "the fast model",
        ModelTier::Medium => "the standard model",
        ModelTier::Complex => "the strongest model",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig() -> Signals {
        Signals::default()
    }

    #[test]
    fn no_signals_at_all_means_medium() {
        // The do-no-harm property. Turning Auto on must not move a card that
        // says nothing about itself — Medium is what it would have got.
        let d = classify(&sig(), Phase::Single);
        assert_eq!(d.tier, ModelTier::Medium);
        assert_eq!(d.rule, "no_signal");
    }

    #[test]
    fn a_one_line_card_with_nothing_attached_goes_cheap() {
        let s = Signals {
            brief_chars: 60,
            ..sig()
        };
        let d = classify(&s, Phase::Single);
        assert_eq!(d.tier, ModelTier::Easy);
        assert_eq!(d.rule, "small_and_unreferenced");
    }

    #[test]
    fn an_all_but_empty_brief_buys_nothing() {
        // The absence of information is not evidence of simplicity. A card
        // with nothing written on it is under-specified, which is harder than
        // a described one — so it must fall through to Medium rather than
        // satisfying "short and unreferenced" by saying nothing at all.
        for chars in [0, 1, MIN_BRIEF - 1] {
            let d = classify(
                &Signals {
                    brief_chars: chars,
                    ..sig()
                },
                Phase::Single,
            );
            assert_eq!(d.tier, ModelTier::Medium, "{chars} chars");
            assert_eq!(d.rule, "no_signal");
        }
        // And the first length that *has* said something does buy Easy.
        let d = classify(
            &Signals {
                brief_chars: MIN_BRIEF,
                ..sig()
            },
            Phase::Single,
        );
        assert_eq!(d.tier, ModelTier::Easy);
    }

    #[test]
    fn shortness_alone_does_not_buy_cheap() {
        // A terse card with a runbook attached is not a small job — the
        // brevity is the *prompt* being short, not the work.
        let s = Signals {
            brief_chars: 60,
            kb_articles: 1,
            ..sig()
        };
        assert_eq!(classify(&s, Phase::Single).tier, ModelTier::Medium);
    }

    #[test]
    fn a_long_and_heavily_referenced_brief_goes_strong() {
        let s = Signals {
            brief_chars: 4_000,
            attachments: 2,
            kb_articles: 2,
            ..sig()
        };
        let d = classify(&s, Phase::Single);
        assert_eq!(d.tier, ModelTier::Complex);
        assert_eq!(d.rule, "heavily_briefed");
    }

    #[test]
    fn planning_gets_the_better_model_and_the_work_pass_does_not() {
        // The biggest legitimate saving in the feature: writing the plan and
        // carrying it out are different jobs, and only one of them needs the
        // expensive model.
        assert_eq!(classify(&sig(), Phase::Plan).tier, ModelTier::Complex);
        let work = classify(&sig(), Phase::Work);
        assert_eq!(work.tier, ModelTier::Medium);
        assert_eq!(work.rule, "executing_approved_plan");
    }

    #[test]
    fn a_retry_never_routes_below_what_already_failed() {
        // Rerunning a failure on a cheaper model pays twice to lose twice.
        for (prior, expected) in [
            (ModelTier::Easy, ModelTier::Medium),
            (ModelTier::Medium, ModelTier::Complex),
            (ModelTier::Complex, ModelTier::Complex),
        ] {
            let s = Signals {
                prior_failed: true,
                prior_tier: Some(prior),
                ..sig()
            };
            assert_eq!(classify(&s, Phase::Single).tier, expected, "from {prior:?}");
        }
    }

    #[test]
    fn a_retry_outranks_every_cheapening_signal() {
        // A one-line card that failed must not go back to Easy just because
        // it is still a one-line card.
        let s = Signals {
            brief_chars: 20,
            prior_failed: true,
            prior_tier: Some(ModelTier::Easy),
            ..sig()
        };
        let d = classify(&s, Phase::Single);
        assert_eq!(d.tier, ModelTier::Medium);
        assert_eq!(d.rule, "retry_escalation");
    }

    #[test]
    fn a_retry_of_a_planning_pass_still_escalates() {
        // Ordering check: the retry rule sits above the phase rules, so a
        // failed plan pass does not silently fall through to planning_pass.
        let s = Signals {
            prior_failed: true,
            prior_tier: Some(ModelTier::Medium),
            ..sig()
        };
        assert_eq!(classify(&s, Phase::Plan).rule, "retry_escalation");
    }

    #[test]
    fn replanned_work_is_treated_as_unclear() {
        let s = Signals {
            replans: 1,
            ..sig()
        };
        let d = classify(&s, Phase::Single);
        assert_eq!(d.tier, ModelTier::Complex);
        assert_eq!(d.rule, "replanned");
    }

    #[test]
    fn every_decision_explains_itself() {
        // The reason is shown on the card, so a branch that produced an empty
        // one would render as aichip having chosen for no stated reason.
        let cases = [
            (sig(), Phase::Single),
            (sig(), Phase::Plan),
            (sig(), Phase::Work),
            (
                Signals {
                    brief_chars: 10,
                    ..sig()
                },
                Phase::Single,
            ),
            (
                Signals {
                    replans: 2,
                    ..sig()
                },
                Phase::Single,
            ),
            (
                Signals {
                    prior_failed: true,
                    ..sig()
                },
                Phase::Single,
            ),
            (
                Signals {
                    brief_chars: 9_000,
                    attachments: 4,
                    ..sig()
                },
                Phase::Single,
            ),
        ];
        for (s, phase) in cases {
            let d = classify(&s, phase);
            assert!(!d.because.is_empty(), "{} gave no reason", d.rule);
            assert!(!d.rule.is_empty());
        }
    }

    #[test]
    fn the_same_signals_always_give_the_same_answer() {
        // No clock, no randomness — a card that reruns unchanged must not
        // wander between tiers.
        let s = Signals {
            brief_chars: 300,
            kb_articles: 1,
            ..sig()
        };
        let first = classify(&s, Phase::Single);
        for _ in 0..5 {
            assert_eq!(classify(&s, Phase::Single), first);
        }
    }
}
