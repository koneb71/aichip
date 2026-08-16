//! Adjusting the plan while the work is happening.
//!
//! Two conversations with the manager: after each hand-off ("does anything
//! still queued need to change?") and after a failure ("what now?"). Both
//! are deliberately small — the common answer to the first is `{}`.
//!
//! The invariants live here in code rather than in the prompt. A model that
//! tries to rewrite finished work, invent a teammate, or add assignments
//! forever gets refused by `apply_decision`, not by wording.

use super::plan::{inspect_plan, resolve_assignee, Plan, PlannedTask, Severity};
use super::roster::Member;
use super::Assignment;
use serde::Deserialize;
use uuid::Uuid;

/// Ceilings on how far a run can drift from its original plan.
pub const MAX_TOTAL_ASSIGNMENTS: usize = 20;
pub const MAX_ADDED_ASSIGNMENTS: usize = 6;
pub const MAX_REPLANS: i32 = 12;
/// How much of a report the manager reads back. Enough to judge, small
/// enough that the cost of re-planning stays flat as the run grows.
pub const REPLAN_OUTCOME_CHARS: usize = 800;

#[derive(Debug, Default, Deserialize)]
pub struct ReplanDecision {
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub drop: Vec<String>,
    #[serde(default)]
    pub revise: Vec<Revision>,
    #[serde(default)]
    pub add: Vec<PlannedTask>,
}

#[derive(Debug, Deserialize)]
pub struct Revision {
    pub key: String,
    #[serde(default)]
    pub brief: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub done_when: Option<Vec<String>>,
}

impl ReplanDecision {
    pub fn is_noop(&self) -> bool {
        self.drop.is_empty() && self.revise.is_empty() && self.add.is_empty()
    }
}

#[derive(Debug, PartialEq)]
pub enum Mutation {
    Drop {
        step_id: Uuid,
        key: String,
    },
    Revise {
        step_id: Uuid,
        key: String,
        assignee: Option<String>,
        brief: Option<String>,
        done_when: Option<Vec<String>>,
    },
    Add(PlannedTask),
}

/// Turn a decision into mutations the executor may safely apply, plus notes
/// about anything refused. Pure.
pub fn apply_decision(
    decision: ReplanDecision,
    pending: &[Assignment],
    roster: &[Member],
    total_assignments: usize,
    add_budget: usize,
) -> (Vec<Mutation>, Vec<String>) {
    let mut mutations = vec![];
    let mut refused = vec![];
    let find = |key: &str| pending.iter().find(|a| a.key == key.trim());

    for key in &decision.drop {
        match find(key) {
            // Only queued work can be dropped: anything else is finished or
            // running, and rewriting history is not on offer.
            Some(a) => mutations.push(Mutation::Drop {
                step_id: a.step_id,
                key: a.key.clone(),
            }),
            None => refused.push(format!("cannot drop \"{key}\" — it isn't still queued")),
        }
    }

    for revision in &decision.revise {
        let Some(assignment) = find(&revision.key) else {
            refused.push(format!(
                "cannot revise \"{}\" — it isn't still queued",
                revision.key
            ));
            continue;
        };
        let assignee = match &revision.assignee {
            Some(name) => match resolve_assignee(roster, name) {
                Some(resolved) => Some(resolved),
                None => {
                    refused.push(format!(
                        "\"{}\" is not on this team, so \"{}\" was left with {}",
                        name, assignment.key, assignment.assignee
                    ));
                    None
                }
            },
            None => None,
        };
        mutations.push(Mutation::Revise {
            step_id: assignment.step_id,
            key: assignment.key.clone(),
            assignee,
            brief: revision.brief.clone().filter(|b| !b.trim().is_empty()),
            done_when: revision.done_when.clone(),
        });
    }

    let dropped = decision.drop.len();
    let room = MAX_TOTAL_ASSIGNMENTS
        .saturating_sub(total_assignments.saturating_sub(dropped))
        .min(add_budget);
    let mut existing_keys: Vec<String> = pending.iter().map(|a| a.key.clone()).collect();

    for task in decision.add.into_iter() {
        if mutations
            .iter()
            .filter(|m| matches!(m, Mutation::Add(_)))
            .count()
            >= room
        {
            refused.push(format!(
                "no room to add \"{}\" — this run is at its assignment ceiling",
                task.key
            ));
            continue;
        }
        let mut task = task;
        task.key = unique_key(&task.key, &existing_keys);

        // A new assignment has to clear the same bar as an original one; a
        // defective one is dropped with a note rather than triggering a
        // whole repair round mid-run.
        let probe = Plan {
            summary: String::new(),
            inspected: vec!["(mid-run addition)".into()],
            tasks: vec![task.clone()],
        };
        let blocking: Vec<String> = inspect_plan(&probe, roster)
            .into_iter()
            .filter(|d| d.severity() == Severity::Blocking)
            .map(|d| d.message())
            .collect();
        if !blocking.is_empty() {
            refused.push(format!(
                "did not add \"{}\": {}",
                task.key,
                blocking.join(" ")
            ));
            continue;
        }
        existing_keys.push(task.key.clone());
        mutations.push(Mutation::Add(task));
    }

    (mutations, refused)
}

/// `api` → `api_2` when the key is already taken. The unique index on
/// (run_id, step_key) would otherwise turn a duplicate into a 500.
fn unique_key(desired: &str, taken: &[String]) -> String {
    let base = desired.trim();
    let base = if base.is_empty() { "task" } else { base };
    if !taken.iter().any(|k| k == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|candidate| !taken.iter().any(|k| k == candidate))
        .expect("an unused suffix exists")
}

// ── Failure triage ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Triage {
    pub action: TriageAction,
    #[serde(default)]
    pub brief: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageAction {
    Retry,
    Reassign,
    Drop,
    Abort,
}

impl Triage {
    /// Aborting is what the executor used to do unconditionally, so an
    /// unreadable answer falling back to it can never be a regression.
    pub fn parse(output: &str) -> Triage {
        crate::runs::utility::extract_json(output)
            .ok()
            .and_then(|v| serde_json::from_value::<Triage>(v).ok())
            .unwrap_or(Triage {
                action: TriageAction::Abort,
                brief: None,
                assignee: None,
                note: String::new(),
            })
    }
}

// ── Prompts ────────────────────────────────────────────────────────────────

pub fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let clipped: String = text.chars().take(max_chars).collect();
    format!("{}…", clipped.trim_end())
}

/// `finished_title` is one title, or several joined when a batch finished
/// together — a parallel batch reports as one hand-off.
pub fn replan_prompt(
    who: &str,
    finished_title: &str,
    outcome: &str,
    pending: &[Assignment],
    add_budget: usize,
) -> String {
    let queued = pending
        .iter()
        .map(|a| {
            format!(
                "- {}: {} → {} ({})",
                a.key,
                a.title,
                a.assignee,
                a.size.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{who} just finished \"{finished_title}\".\n\n\
         What they reported:\n{}\n\n\
         Still queued ({} left):\n{queued}\n\n\
         Adjust only what has not started. Finished work is done and cannot be changed.\n\n\
         Reply with ONLY a JSON object. If nothing needs to change — the usual case — reply \
         exactly {{}} and nothing else.\n\n\
         {{\"note\": \"one line for the team feed, only if you changed something\",\n\
         \x20\"drop\": [\"keys that are no longer needed\"],\n\
         \x20\"revise\": [{{\"key\": \"…\", \"brief\": \"…\", \"assignee\": \"…\", \"done_when\": [\"…\"]}}],\n\
         \x20\"add\": [{{\"key\": \"…\", \"title\": \"…\", \"brief\": \"…\", \"assignee\": \"…\", \
         \"done_when\": [\"…\"], \"size\": \"small\", \"depends_on\": [\"…\"], \
         \"touches\": [\"paths it will change\"]}}]}}\n\n\
         Add work only when that report revealed something the plan genuinely missed — not to \
         polish, not to gold-plate. You may add at most {add_budget} more assignments for the rest \
         of this run.",
        clip(outcome, REPLAN_OUTCOME_CHARS),
        pending.len(),
    )
}

pub fn triage_prompt(who: &str, title: &str, reason: &str, output: &str) -> String {
    format!(
        "{who} could not finish \"{title}\".\n\n\
         What went wrong:\n{reason}\n\n\
         Their last report:\n{}\n\n\
         Decide, and reply with ONLY a JSON object:\n\
         {{\"action\": \"retry\" | \"reassign\" | \"drop\" | \"abort\",\n\
         \x20\"brief\": \"revised brief — required for retry and reassign\",\n\
         \x20\"assignee\": \"specialist name — required for reassign\",\n\
         \x20\"note\": \"one line for the team feed\"}}\n\n\
         retry — same person, a clearer or smaller brief\n\
         reassign — someone better suited, with a brief written for them\n\
         drop — the goal survives without this; the rest of the plan continues\n\
         abort — nothing useful can come of this run; stop now",
        clip(output, REPLAN_OUTCOME_CHARS)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::org::plan::TaskSize;
    use aichip_shared::ModelTier;

    fn member(name: &str) -> Member {
        Member {
            agent_id: Uuid::new_v4(),
            name: name.into(),
            title: "Specialist".into(),
            description: "does things".into(),
            tier: ModelTier::Medium,
            effort: None,
            system_prompt: String::new(),
        }
    }

    fn assignment(key: &str, assignee: &str) -> Assignment {
        Assignment {
            step_id: Uuid::new_v4(),
            key: key.into(),
            title: format!("Do {key}"),
            brief: "b".repeat(120),
            assignee: assignee.into(),
            done_when: vec!["ok".into()],
            size: TaskSize::Medium,
            depends_on: vec![],
            touches: vec![],
            status: "queued".into(),
            output: None,
            attempt: 1,
        }
    }

    fn addable(key: &str, assignee: &str) -> PlannedTask {
        PlannedTask {
            key: key.into(),
            title: format!("Do {key}"),
            brief: "b".repeat(120),
            assignee: assignee.into(),
            done_when: vec!["ok".into()],
            size: TaskSize::Small,
            depends_on: vec![],
            touches: vec![],
        }
    }

    #[test]
    fn an_empty_object_is_the_cheap_no_change_answer() {
        let decision: ReplanDecision = serde_json::from_str("{}").unwrap();
        assert!(decision.is_noop());
    }

    #[test]
    fn finished_work_cannot_be_rewritten() {
        let pending = vec![assignment("ui", "Rex")];
        let decision = ReplanDecision {
            drop: vec!["api".into()], // already completed, not in pending
            revise: vec![Revision {
                key: "api".into(),
                brief: Some("changed".into()),
                assignee: None,
                done_when: None,
            }],
            ..Default::default()
        };
        let (mutations, refused) = apply_decision(
            decision,
            &pending,
            &[member("Rex")],
            2,
            MAX_ADDED_ASSIGNMENTS,
        );
        assert!(mutations.is_empty());
        assert_eq!(refused.len(), 2);
        assert!(refused[0].contains("isn't still queued"));
    }

    #[test]
    fn an_invented_teammate_is_refused_rather_than_misrouted() {
        let pending = vec![assignment("ui", "Rex")];
        let decision = ReplanDecision {
            revise: vec![Revision {
                key: "ui".into(),
                brief: None,
                assignee: Some("Ghost".into()),
                done_when: None,
            }],
            add: vec![addable("extra", "Ghost")],
            ..Default::default()
        };
        let (mutations, refused) = apply_decision(
            decision,
            &pending,
            &[member("Rex")],
            1,
            MAX_ADDED_ASSIGNMENTS,
        );

        // The revision survives, but the bogus assignee is dropped from it.
        match &mutations[0] {
            Mutation::Revise { assignee, .. } => assert!(assignee.is_none()),
            other => panic!("expected a revision, got {other:?}"),
        }
        assert!(!mutations.iter().any(|m| matches!(m, Mutation::Add(_))));
        assert_eq!(refused.len(), 2);
    }

    #[test]
    fn additions_stop_at_the_run_ceiling() {
        let pending = vec![assignment("ui", "Rex")];
        let decision = ReplanDecision {
            add: (0..5).map(|i| addable(&format!("x{i}"), "Rex")).collect(),
            ..Default::default()
        };
        let (mutations, refused) = apply_decision(
            decision,
            &pending,
            &[member("Rex")],
            MAX_TOTAL_ASSIGNMENTS - 2,
            MAX_ADDED_ASSIGNMENTS,
        );
        assert_eq!(mutations.len(), 2, "only the remaining room is used");
        assert!(refused.iter().any(|r| r.contains("ceiling")));
    }

    #[test]
    fn the_add_budget_caps_a_single_round() {
        let pending = vec![assignment("ui", "Rex")];
        let decision = ReplanDecision {
            add: (0..4).map(|i| addable(&format!("x{i}"), "Rex")).collect(),
            ..Default::default()
        };
        let (mutations, _) = apply_decision(decision, &pending, &[member("Rex")], 1, 2);
        assert_eq!(mutations.len(), 2);
    }

    #[test]
    fn a_colliding_key_is_suffixed_not_rejected() {
        let pending = vec![assignment("api", "Rex")];
        let decision = ReplanDecision {
            add: vec![addable("api", "Rex")],
            ..Default::default()
        };
        let (mutations, _) = apply_decision(
            decision,
            &pending,
            &[member("Rex")],
            1,
            MAX_ADDED_ASSIGNMENTS,
        );
        match &mutations[0] {
            Mutation::Add(task) => assert_eq!(task.key, "api_2"),
            other => panic!("expected an addition, got {other:?}"),
        }
    }

    #[test]
    fn a_defective_addition_is_skipped_with_a_reason() {
        let pending = vec![assignment("ui", "Rex")];
        let mut bad = addable("thin", "Rex");
        bad.brief = "too short".into();
        let decision = ReplanDecision {
            add: vec![bad],
            ..Default::default()
        };
        let (mutations, refused) = apply_decision(
            decision,
            &pending,
            &[member("Rex")],
            1,
            MAX_ADDED_ASSIGNMENTS,
        );
        assert!(mutations.is_empty());
        assert!(refused[0].contains("did not add"));
    }

    #[test]
    fn triage_parses_every_action_and_defaults_to_abort() {
        for (raw, expected) in [
            (
                r#"{"action":"retry","brief":"smaller"}"#,
                TriageAction::Retry,
            ),
            (
                r#"{"action":"reassign","assignee":"Rex"}"#,
                TriageAction::Reassign,
            ),
            (r#"{"action":"drop"}"#, TriageAction::Drop),
            (r#"{"action":"abort"}"#, TriageAction::Abort),
        ] {
            assert_eq!(Triage::parse(raw).action, expected);
        }
        assert_eq!(Triage::parse("I give up").action, TriageAction::Abort);
        assert_eq!(
            Triage::parse(r#"{"action":"nap"}"#).action,
            TriageAction::Abort
        );
    }

    #[test]
    fn the_replan_prompt_stays_small_as_the_run_grows() {
        let pending = vec![assignment("ui", "Rex")];
        let prompt = replan_prompt("Priya", "Add API", &"x".repeat(5000), &pending, 3);
        assert!(prompt.contains('…'), "a long report is clipped");
        assert!(
            prompt.len() < 3000,
            "prompt stayed compact: {}",
            prompt.len()
        );
        assert!(prompt.contains("reply exactly {} and nothing else"));
        assert!(prompt.contains("at most 3 more assignments"));
        // Titles only — never the full briefs of everything still queued.
        assert!(!prompt.contains(&"b".repeat(120)));
    }
}
