//! Plan mode for the assistant chat: research and propose, do not act.
//!
//! The chat's tools are mostly read-only, and four of them are not:
//! `create_task`, `start_task`, `move_task` and `cancel_task`. Those four are
//! how a misunderstanding turns into money — the assistant creates a card,
//! assigns an agent and starts it, and the first sign it had the wrong idea is
//! a run that has already spent. Plan mode takes those four away for a turn,
//! asks for a plan instead, and gives the person a button.
//!
//! **Not the card's shape, and deliberately.** `task_plan` parks the run and
//! re-queues it on approval, because a parked run would otherwise sit on one
//! of a small number of concurrency permits while somebody reads. A chat turn
//! cannot park at all: `routes::chat::active_run` counts every non-terminal
//! run as active and refuses the next message, so a parked plan would freeze
//! the conversation it is meant to be part of. The plan turn therefore
//! *finishes*, like any other reply, and approving sends a new turn. That is
//! also how it reads: a plan you can argue with in the next sentence.
//!
//! Everything here is pure, so the prompts and the tool arithmetic can be
//! tested without a database, an engine or a chat.

/// The tools plan mode takes away.
///
/// Exactly the mutating ones. `list_tasks`, `get_diff`, `get_spend` and the
/// rest stay: a plan is worth more when the thing writing it has read the
/// board first, and none of them change anything.
pub const ACTING_TOOLS: &[&str] = &[
    "mcp__aichip__create_task",
    "mcp__aichip__start_task",
    "mcp__aichip__move_task",
    "mcp__aichip__cancel_task",
];

/// The same four, as the MCP server advertises them — without the prefix the
/// engine adds.
pub const ACTING_TOOL_NAMES: &[&str] = &["create_task", "start_task", "move_task", "cancel_task"];

/// `allowed` with the acting tools removed.
///
/// Order is preserved so the remaining list stays the one the pinned test
/// describes, minus the four.
pub fn without_acting(allowed: &[String]) -> Vec<String> {
    allowed
        .iter()
        .filter(|t| !ACTING_TOOLS.contains(&t.as_str()))
        .cloned()
        .collect()
}

/// `denied` with the acting tools added.
///
/// Both halves, and that is not belt-and-braces for its own sake: CLAUDE.md
/// records that `allowed_tools` is an *auto-approval* list rather than a
/// restriction — Claude Code will still reach for a tool that is merely absent
/// from it. `denied_tools` is what actually stops the call. Dropping it from
/// `allowed` as well is what keeps the advertised toolbox honest, so the
/// assistant never tries.
pub fn with_acting_denied(denied: &[String]) -> Vec<String> {
    let mut out = denied.to_vec();
    for t in ACTING_TOOLS {
        if !out.iter().any(|d| d == t) {
            out.push(t.to_string());
        }
    }
    out
}

/// What a plan-mode turn asks for, appended to the person's message.
///
/// Appended rather than replacing the system prompt: the system prompt is the
/// assistant's standing brief and is carried by a resumed session, while plan
/// mode is a property of *this* turn. It says what the assistant cannot do as
/// well as what it should, because a model that discovers a tool is missing
/// halfway through tends to report the workspace as broken rather than to
/// carry on writing.
pub fn instruction() -> &'static str {
    "\n\n---\n\n**Plan mode is on for this message.** Research freely — read \
     files, search the code, look at the board — then write down what you \
     propose to do, and stop.\n\n\
     You cannot create, start, move or cancel a card this turn; those tools \
     are switched off deliberately, so do not try them and do not report \
     their absence as a fault. Nothing you write will happen until the person \
     approves it.\n\n\
     **If a choice would change the plan, ask before writing it.** Call \
     mcp__aichip__ask_user with the readings as options and stop there — a \
     question is cheap and a plan built on the wrong assumption wastes the \
     whole turn, plus the approval that follows it. Make the options genuinely \
     different approaches, not degrees of the same one, and give each the \
     one line that says what picking it means. Ask about what you cannot \
     settle by reading the code; do not ask for permission to proceed, and do \
     not ask about something the person already told you. Answering keeps you \
     in plan mode, so you carry on planning with the answer in hand.\n\n\
     **One round of questions is usually enough.** If you have already asked \
     in this conversation, plan against your best reading and put what is \
     still open at the bottom, rather than asking again — ask a second time \
     only when the answer you got opened a fork you could not have seen \
     before. A plan they can argue with beats a third question.\n\n\
     Once the choices are settled, write the plan as a short numbered list of \
     the cards you would create — each with the title you would give it and \
     one line on what it covers — followed by anything still uncertain that \
     was not worth a question. If the request turns out to need no cards at \
     all, say that instead of inventing some."
}

/// What approving a plan sends back.
///
/// The plan text is included only when the person changed it. The session is
/// resumed, so the assistant already has the plan it wrote; repeating it
/// unchanged would spend the tokens again and, worse, invite it to treat its
/// own proposal as a fresh instruction from the user. An *edited* plan is a
/// different matter — it has to be told which version is authoritative, the
/// same reason `runs.plan_edited` exists on the card path.
pub fn approval(edited: Option<&str>) -> String {
    match edited {
        None => "Approved — carry out the plan exactly as you wrote it. \
                 Create and start the cards you listed, and nothing else."
            .to_string(),
        Some(plan) => format!(
            "Approved, with edits. This is the authoritative version — where it \
             differs from what you proposed, this wins. Carry it out exactly, \
             and do nothing that is not in it.\n\n{plan}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn planning_removes_exactly_the_four_that_change_something() {
        let allowed = strings(&[
            "Read",
            "Grep",
            "mcp__aichip__create_task",
            "mcp__aichip__start_task",
            "mcp__aichip__list_tasks",
            "mcp__aichip__cancel_task",
            "mcp__aichip__get_diff",
            "mcp__aichip__move_task",
            "mcp__aichip__search_code",
        ]);
        assert_eq!(
            without_acting(&allowed),
            strings(&[
                "Read",
                "Grep",
                "mcp__aichip__list_tasks",
                "mcp__aichip__get_diff",
                "mcp__aichip__search_code",
            ])
        );
    }

    #[test]
    fn reading_the_board_survives_plan_mode() {
        // A plan written without looking is a guess. Every read-only tool has
        // to still be there.
        let allowed = strings(&[
            "mcp__aichip__list_tasks",
            "mcp__aichip__get_task_status",
            "mcp__aichip__list_agents",
            "mcp__aichip__get_spend",
            "mcp__aichip__list_skills",
            "mcp__aichip__search_code",
        ]);
        assert_eq!(without_acting(&allowed), allowed);
    }

    #[test]
    fn denying_is_additive_and_never_duplicates() {
        let denied = strings(&["Edit", "Write", "Bash"]);
        let out = with_acting_denied(&denied);
        for t in ACTING_TOOLS {
            assert_eq!(out.iter().filter(|d| d.as_str() == *t).count(), 1, "{t}");
        }
        // The originals survive: plan mode adds to the write ban, it does not
        // replace it.
        for t in ["Edit", "Write", "Bash"] {
            assert!(out.iter().any(|d| d == t), "{t} was dropped");
        }
        assert_eq!(
            with_acting_denied(&out),
            out,
            "applying it twice must not grow it"
        );
    }

    #[test]
    fn the_two_lists_describe_the_same_four_tools() {
        // One is prefixed the way the engine names them, the other the way the
        // MCP server advertises them. They must not drift.
        assert_eq!(ACTING_TOOLS.len(), ACTING_TOOL_NAMES.len());
        for (full, short) in ACTING_TOOLS.iter().zip(ACTING_TOOL_NAMES) {
            assert_eq!(*full, format!("mcp__aichip__{short}"));
        }
    }

    #[test]
    fn the_instruction_says_what_is_switched_off_and_that_nothing_happens_yet() {
        let t = instruction();
        assert!(t.contains("Plan mode is on"));
        // Naming the missing tools is what stops the model reporting them as
        // a broken workspace.
        for word in ["create", "start", "move", "cancel"] {
            assert!(t.contains(word), "the instruction never mentions {word}");
        }
        assert!(t.contains("until the person approves"));
    }

    #[test]
    fn the_instruction_asks_before_it_plans() {
        // The tool was always available in plan mode — `tools_list` keeps it
        // out of the acting filter deliberately — but the instruction used to
        // steer the other way, telling the assistant to write its uncertainty
        // down as prose at the end. So it never asked, and a plan built on the
        // wrong reading was only discovered at approval.
        let t = instruction();
        assert!(t.contains("ask_user"));
        // Before, not after: a question asked underneath a finished plan is a
        // question about a plan that already assumed an answer.
        let ask = t.find("ask_user").expect("names the tool");
        let write = t.find("write the plan").expect("still asks for a plan");
        assert!(
            ask < write,
            "the instruction asks for the plan before it allows a question"
        );
        // What makes the options worth clicking rather than a formality.
        assert!(t.contains("genuinely different"));
        // And the thing a model cannot infer: its turn ends, but the mode does
        // not, so it should expect to carry on planning afterwards.
        assert!(t.contains("keeps you in plan mode"));
    }

    #[test]
    fn the_instruction_bounds_how_long_it_can_keep_asking() {
        // Nothing in the machinery stops an assistant asking every turn: the
        // question ends the turn, the answer starts another plan turn, and
        // round trips are free to it and not to the person paying for them.
        // The bound is a soft one on purpose — the second question is
        // sometimes the useful one — but it has to exist.
        let t = instruction();
        assert!(t.contains("One round of questions is usually enough"));
        assert!(t.contains("already asked"));
    }

    #[test]
    fn ask_user_is_not_one_of_the_tools_plan_mode_takes_away() {
        // The whole feature rests on this: asking has to survive the filter
        // that removes the acting tools.
        assert!(!ACTING_TOOLS.contains(&"mcp__aichip__ask_user"));
        assert!(!ACTING_TOOL_NAMES.contains(&"ask_user"));
        let allowed = strings(&["mcp__aichip__ask_user", "mcp__aichip__create_task"]);
        assert_eq!(
            without_acting(&allowed),
            strings(&["mcp__aichip__ask_user"])
        );
    }

    #[test]
    fn an_unedited_approval_does_not_repeat_the_plan_back() {
        let a = approval(None);
        assert!(a.contains("exactly as you wrote it"));
        assert!(!a.contains('\n'), "an unedited approval is one line: {a}");
    }

    #[test]
    fn an_edited_approval_says_which_version_wins() {
        let a = approval(Some("1. Only do the first thing"));
        assert!(a.contains("this wins"));
        assert!(a.ends_with("1. Only do the first thing"));
    }
}
