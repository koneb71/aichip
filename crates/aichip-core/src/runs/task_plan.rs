//! Plan-first task runs: look before you leap.
//!
//! A card can ask the agent to write down what it intends to do *before* it
//! touches anything. The run parks, you read the plan, edit it or send it back
//! for another pass, and only then does work start.
//!
//! Why this is two dispatches rather than one run that waits: a parked run
//! holds its concurrency permit for the whole of `execute`, so pausing
//! in-place would eat one of a small number of slots for however long a person
//! takes to read. Planning therefore *finishes*, the run parks, and approving
//! re-queues it — the same shape organizations already use for their plans.
//!
//! Everything here is pure so the prompts and the phase decision can be tested
//! without a database, an engine, or a worktree.

/// Which half of a plan-first run this dispatch is.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Write the plan, then park.
    Plan,
    /// Do the work. Carries the plan the user actually approved, which is not
    /// necessarily the one the agent wrote.
    Work { plan: Option<String> },
}

/// What a stored plan step looks like to the decision below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlanStep<'a> {
    /// No plan has been written yet.
    Missing,
    /// A plan exists, with its text.
    Written(&'a str),
}

/// Decide which half to run.
///
/// The `approved` flag is deliberately separate from "a plan exists": a plan
/// that was written but not yet approved must **not** fall through to work.
/// That is the entire point of the feature, and getting it backwards would
/// silently do the thing the user asked to see first.
pub fn decide(plan_first: bool, step: PlanStep<'_>, approved: bool) -> Phase {
    if !plan_first {
        return Phase::Work { plan: None };
    }
    match step {
        PlanStep::Missing => Phase::Plan,
        // Written but unapproved: re-plan rather than run. Reaching here means
        // the run was re-queued without going through approval — a revise
        // request, or a crash between writing and parking.
        PlanStep::Written(_) if !approved => Phase::Plan,
        PlanStep::Written(plan) => Phase::Work {
            plan: Some(plan.to_string()),
        },
    }
}

/// Tools the planning pass may use. Read-only on purpose: a plan you are going
/// to be asked to approve is worthless if the work already happened while it
/// was being written.
pub const PLANNING_TOOLS: &[&str] = &["Read", "Grep", "Glob"];

/// Tools the planning pass must not reach for, whatever else is granted.
///
/// Naming the read-only three above is **not** enough. Claude Code's
/// `--allowedTools` is an auto-approval list, not a restriction — a planning
/// pass allowed only `Read` still ran `Bash` here, which is how this constant
/// came to exist. Denial is the part that binds.
///
/// `Bash` is on the list even though `git log` would genuinely help a plan:
/// one `>` redirect and the work has happened. The promise being kept is
/// "nothing changed", and it has to hold against a shell.
pub const PLANNING_DENIED: &[&str] =
    &["Edit", "Write", "MultiEdit", "NotebookEdit", "Bash", "WebFetch"];

/// Ask for a plan, not for the work.
pub fn plan_prompt(task_prompt: &str) -> String {
    format!(
        "{task_prompt}\n\n\
         ---\n\
         **Do not make any changes yet.** Your only job right now is to write the plan \
         that a person will read and approve before you start.\n\n\
         You can read files, grep, and glob — you cannot run commands or change \
         anything, so don't plan around doing so during this pass.\n\n\
         Read whatever you need to read first, then reply with:\n\n\
         1. **What you found** — one short paragraph on how the relevant code works today. \
         Say plainly if something you expected isn't there.\n\
         2. **What you'll change** — a numbered list. Name the actual files. One line each, \
         concrete enough that someone who knows this codebase could tell you're wrong.\n\
         3. **What you won't touch** — anything nearby you're deliberately leaving alone.\n\
         4. **Anything you're unsure about** — where you had to guess, and what you'd \
         assume if nobody answers. Say \"nothing\" if there genuinely is nothing.\n\n\
         Keep it under 300 words. No code blocks unless a single line settles a question."
    )
}

/// Turn an approved plan into the brief for the work pass.
///
/// `edited` matters: when a person has rewritten the plan, the agent is
/// resuming a session in which it remembers proposing something *else*. Left
/// unsaid, it will reasonably follow its own memory over the text in front of
/// it — so the difference is called out rather than hoped about.
pub fn work_prompt(task_prompt: &str, plan: &str, edited: bool) -> String {
    let preamble = if edited {
        "Your plan has been **edited and approved**. The version below is what was approved — \
         it is not identical to what you proposed. Where they differ, the version below wins, \
         even where you preferred your own. Don't argue the difference; if a change makes \
         something impossible, do the rest and say so at the end."
    } else {
        "Your plan has been **approved as written**. Follow it."
    };
    format!(
        "{preamble}\n\n\
         ## Approved plan\n\n{plan}\n\n\
         ---\n\n\
         Now do the work. The original request, for reference:\n\n{task_prompt}\n\n\
         If you discover partway through that the plan was wrong, finish what still makes \
         sense, stop before doing anything it didn't cover, and say what you hit."
    )
}

/// Ask for another pass, given what the person didn't like.
pub fn revise_prompt(task_prompt: &str, previous: &str, note: &str) -> String {
    format!(
        "Your plan wasn't approved. Here's the feedback:\n\n> {note}\n\n\
         ## What you proposed\n\n{previous}\n\n\
         ---\n\n\
         Write a new plan that answers the feedback. Same format and same limit as before, \
         and still **make no changes**. If the feedback rests on something you believe is \
         mistaken, say so in one sentence and plan the version you'd defend — being agreeable \
         about a wrong instruction wastes the next round too.\n\n\
         The original request:\n\n{task_prompt}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_without_plan_first_goes_straight_to_work() {
        assert_eq!(
            decide(false, PlanStep::Missing, false),
            Phase::Work { plan: None }
        );
    }

    #[test]
    fn a_plan_first_card_plans_before_it_works() {
        assert_eq!(decide(true, PlanStep::Missing, false), Phase::Plan);
    }

    /// The whole feature is this line. A written-but-unapproved plan falling
    /// through to work would do the thing the user asked to see first.
    #[test]
    fn a_written_plan_is_not_a_licence_to_start() {
        assert_eq!(decide(true, PlanStep::Written("do the thing"), false), Phase::Plan);
    }

    #[test]
    fn approval_carries_the_stored_text_into_the_work_pass() {
        // Stored, not remembered: the text may have been rewritten by hand
        // since the agent proposed it.
        assert_eq!(
            decide(true, PlanStep::Written("edited by hand"), true),
            Phase::Work { plan: Some("edited by hand".into()) }
        );
    }

    /// Leaving a tool off the allow-list does not stop it being used — that is
    /// exactly the mistake this feature shipped with. Every mutating tool has
    /// to be *denied*, not merely unlisted.
    #[test]
    fn every_mutating_tool_is_explicitly_denied() {
        for tool in ["Edit", "Write", "MultiEdit", "NotebookEdit", "Bash"] {
            assert!(
                PLANNING_DENIED.contains(&tool),
                "{tool} can change the repo, so being absent from PLANNING_TOOLS \
                 is not enough — it has to be denied"
            );
            assert!(!PLANNING_TOOLS.contains(&tool));
        }
    }

    #[test]
    fn the_two_lists_never_overlap() {
        for tool in PLANNING_TOOLS {
            assert!(
                !PLANNING_DENIED.contains(tool),
                "{tool} is both granted and denied; which wins is the adapter's \
                 guess, and a guess is not a guarantee"
            );
        }
    }

    #[test]
    fn an_edited_plan_says_so_and_says_which_version_wins() {
        let edited = work_prompt("build it", "step one", true);
        assert!(edited.contains("edited"));
        assert!(
            edited.contains("below wins"),
            "resuming a session, the agent remembers proposing something else"
        );

        let verbatim = work_prompt("build it", "step one", false);
        assert!(verbatim.contains("approved as written"));
        assert!(!verbatim.contains("below wins"));
    }

    #[test]
    fn every_prompt_carries_the_original_request() {
        // Losing it would leave the agent working from a summary of a summary.
        let task = "add rate limiting to the login route";
        assert!(plan_prompt(task).contains(task));
        assert!(work_prompt(task, "p", false).contains(task));
        assert!(revise_prompt(task, "p", "too vague").contains(task));
    }

    #[test]
    fn the_planning_prompt_forbids_changes() {
        assert!(plan_prompt("x").contains("Do not make any changes"));
        assert!(revise_prompt("x", "p", "n").contains("make no changes"));
    }
}
