//! What the manager is asked for, and what counts as an answer worth running.
//!
//! Everything here is pure so the interesting cases — a monolithic brief, a
//! hallucinated assignee, a cyclic plan — are testable without a model, a
//! database, or a token.

use super::roster::Member;
use aichip_shared::ModelTier;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Sizing rules. These are what turn "split the work" from an aspiration
/// into something the executor can refuse.
pub const MAX_TASKS: usize = 12;
pub const MIN_BRIEF_CHARS: usize = 80;
pub const MAX_BRIEF_CHARS: usize = 1200;
pub const MAX_LARGE_TASKS: usize = 1;
pub const MAX_DONE_WHEN: usize = 5;
/// One person holding more than this share of the plan is the shape of the
/// failure that prompted all of this.
pub const OVERLOAD_SHARE: f64 = 0.6;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Plan {
    #[serde(default)]
    pub summary: String,
    /// Paths the manager says it actually opened. Making it enumerate them
    /// is what makes it look.
    #[serde(default)]
    pub inspected: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<PlannedTask>,
}

/// Every field defaults on purpose: a missing `brief` should be a defect the
/// manager can fix in one more turn, not a parse error that kills the run.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct PlannedTask {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub done_when: Vec<String>,
    #[serde(default)]
    pub size: TaskSize,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Files or directories this assignment expects to change. Lets the
    /// scheduler run assignments with disjoint scopes at the same time.
    #[serde(default)]
    pub touches: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl TaskSize {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "small" => Self::Small,
            "large" => Self::Large,
            _ => Self::Medium,
        }
    }
}

// ── Defects ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth telling the manager about; never fails a run on its own.
    Advisory,
    /// The plan cannot be executed as written.
    Blocking,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Defect {
    Unparseable(String),
    NoTasks,
    TooManyTasks { count: usize },
    MissingField { key: String, field: &'static str },
    DuplicateKey { key: String },
    UnknownAssignee { key: String, assignee: String },
    BriefTooShort { key: String, chars: usize },
    BriefTooLong { key: String, chars: usize },
    NoDoneWhen { key: String },
    TooManyDoneWhen { key: String, count: usize },
    TooManyLarge { count: usize },
    SelfDependency { key: String },
    UnknownDependency { key: String, dep: String },
    CyclicDependency { keys: Vec<String> },
    NothingInspected,
    IdleSpecialist { name: String },
    OverloadedSpecialist { name: String, count: usize, total: usize },
}

impl Defect {
    pub fn severity(&self) -> Severity {
        match self {
            // Judgement calls: raise them, let the manager decide, never
            // fail a run over them.
            Defect::NothingInspected
            | Defect::IdleSpecialist { .. }
            | Defect::OverloadedSpecialist { .. }
            | Defect::TooManyDoneWhen { .. } => Severity::Advisory,
            _ => Severity::Blocking,
        }
    }

    /// One line, addressed to the manager. This text is the repair prompt.
    pub fn message(&self) -> String {
        match self {
            Defect::Unparseable(why) => {
                format!("I couldn't read that as JSON ({why}). Send the object on its own — no prose, no fences.")
            }
            Defect::NoTasks => "The plan has no assignments in it.".into(),
            Defect::TooManyTasks { count } => format!(
                "{count} assignments is past the ceiling of {MAX_TASKS}. Merge the ones that belong together."
            ),
            Defect::MissingField { key, field } => {
                format!("\"{key}\": no {field}.")
            }
            Defect::DuplicateKey { key } => {
                format!("Two assignments share the key \"{key}\"; keys have to be unique.")
            }
            Defect::UnknownAssignee { key, assignee } => format!(
                "\"{key}\" is assigned to \"{assignee}\", who isn't on this team. Use a name from the roster exactly."
            ),
            Defect::BriefTooShort { key, chars } => format!(
                "\"{key}\": the brief is only {chars} characters. Say what to change, where, and what done looks like."
            ),
            Defect::BriefTooLong { key, chars } => format!(
                "\"{key}\": the brief is {chars} characters. That's a project, not an assignment — split it into sequenced pieces, each with its own done_when."
            ),
            Defect::NoDoneWhen { key } => format!(
                "\"{key}\": no done_when. Give one to {MAX_DONE_WHEN} checkable facts."
            ),
            Defect::TooManyDoneWhen { key, count } => format!(
                "\"{key}\" lists {count} done_when lines; more than {MAX_DONE_WHEN} usually means it's really two assignments."
            ),
            Defect::TooManyLarge { count } => format!(
                "{count} assignments are sized \"large\". At most {MAX_LARGE_TASKS} may be, and only when splitting genuinely isn't possible."
            ),
            Defect::SelfDependency { key } => format!("\"{key}\" depends on itself."),
            Defect::UnknownDependency { key, dep } => {
                format!("\"{key}\" depends on \"{dep}\", which isn't an assignment in this plan.")
            }
            Defect::CyclicDependency { keys } => format!(
                "These depend on each other in a loop and none can start: {}.",
                keys.join(", ")
            ),
            Defect::NothingInspected => "You didn't list anything you looked at. Read the repository before planning against it.".into(),
            Defect::IdleSpecialist { name } => format!(
                "Nobody assigned anything to {name}. If they genuinely have nothing to do here, leave the plan as it is; otherwise give them work."
            ),
            Defect::OverloadedSpecialist { name, count, total } => format!(
                "{name} holds {count} of {total} assignments. Spread the work, or split theirs into smaller sequenced pieces."
            ),
        }
    }
}

/// Parse the manager's reply. Never returns a hard error — an unreadable
/// reply is just the first defect.
pub fn parse_plan(output: &str) -> Result<Plan, Defect> {
    let value = crate::runs::utility::extract_json(output)
        .map_err(|e| Defect::Unparseable(e.to_string()))?;
    serde_json::from_value(value).map_err(|e| Defect::Unparseable(e.to_string()))
}

/// Match a name the way a person would: ignoring case and stray spaces.
pub fn resolve_assignee(members: &[Member], name: &str) -> Option<String> {
    let wanted = name.trim().to_ascii_lowercase();
    members
        .iter()
        .find(|m| m.name.trim().to_ascii_lowercase() == wanted)
        .map(|m| m.name.clone())
}

/// Everything wrong with a plan, worst first.
pub fn inspect_plan(plan: &Plan, roster: &[Member]) -> Vec<Defect> {
    let mut defects = vec![];

    if plan.tasks.is_empty() {
        defects.push(Defect::NoTasks);
        return defects; // nothing else is meaningful
    }
    if plan.tasks.len() > MAX_TASKS {
        defects.push(Defect::TooManyTasks {
            count: plan.tasks.len(),
        });
    }
    if plan.inspected.is_empty() {
        defects.push(Defect::NothingInspected);
    }

    let mut seen_keys: HashSet<&str> = HashSet::new();
    let mut load: HashMap<String, usize> = HashMap::new();
    let mut large = 0;

    for task in &plan.tasks {
        let key = if task.key.trim().is_empty() {
            defects.push(Defect::MissingField {
                key: task.title.clone(),
                field: "key",
            });
            continue;
        } else {
            task.key.trim()
        };
        if !seen_keys.insert(key) {
            defects.push(Defect::DuplicateKey { key: key.into() });
        }
        if task.title.trim().is_empty() {
            defects.push(Defect::MissingField {
                key: key.into(),
                field: "title",
            });
        }

        let brief = task.brief.trim();
        if brief.is_empty() {
            defects.push(Defect::MissingField {
                key: key.into(),
                field: "brief",
            });
        } else if brief.chars().count() < MIN_BRIEF_CHARS {
            defects.push(Defect::BriefTooShort {
                key: key.into(),
                chars: brief.chars().count(),
            });
        } else if brief.chars().count() > MAX_BRIEF_CHARS {
            defects.push(Defect::BriefTooLong {
                key: key.into(),
                chars: brief.chars().count(),
            });
        }

        match resolve_assignee(roster, &task.assignee) {
            Some(name) => *load.entry(name).or_default() += 1,
            None if task.assignee.trim().is_empty() => defects.push(Defect::MissingField {
                key: key.into(),
                field: "assignee",
            }),
            None => defects.push(Defect::UnknownAssignee {
                key: key.into(),
                assignee: task.assignee.clone(),
            }),
        }

        if task.done_when.iter().all(|d| d.trim().is_empty()) {
            defects.push(Defect::NoDoneWhen { key: key.into() });
        } else if task.done_when.len() > MAX_DONE_WHEN {
            defects.push(Defect::TooManyDoneWhen {
                key: key.into(),
                count: task.done_when.len(),
            });
        }

        if task.size == TaskSize::Large {
            large += 1;
        }
        if task.depends_on.iter().any(|d| d.trim() == key) {
            defects.push(Defect::SelfDependency { key: key.into() });
        }
    }

    if large > MAX_LARGE_TASKS {
        defects.push(Defect::TooManyLarge { count: large });
    }

    for task in &plan.tasks {
        for dep in &task.depends_on {
            let dep = dep.trim();
            if !dep.is_empty() && dep != task.key.trim() && !seen_keys.contains(dep) {
                defects.push(Defect::UnknownDependency {
                    key: task.key.clone(),
                    dep: dep.to_string(),
                });
            }
        }
    }
    if let Some(cycle) = find_cycle(&plan.tasks) {
        defects.push(Defect::CyclicDependency { keys: cycle });
    }

    // Workload advisories, once the plan is otherwise understood.
    let total = plan.tasks.len();
    for member in roster {
        match load.get(&member.name) {
            None => defects.push(Defect::IdleSpecialist {
                name: member.name.clone(),
            }),
            Some(&count) if total > 1 && count as f64 / total as f64 > OVERLOAD_SHARE => {
                defects.push(Defect::OverloadedSpecialist {
                    name: member.name.clone(),
                    count,
                    total,
                })
            }
            _ => {}
        }
    }

    defects.sort_by_key(|d| std::cmp::Reverse(d.severity()));
    defects
}

/// Keys caught in a dependency loop, if any.
fn find_cycle(tasks: &[PlannedTask]) -> Option<Vec<String>> {
    let known: HashSet<&str> = tasks.iter().map(|t| t.key.trim()).collect();
    let mut remaining: HashMap<&str, HashSet<&str>> = tasks
        .iter()
        .map(|t| {
            (
                t.key.trim(),
                t.depends_on
                    .iter()
                    .map(|d| d.trim())
                    .filter(|d| known.contains(d) && *d != t.key.trim())
                    .collect(),
            )
        })
        .collect();

    loop {
        let ready: Vec<&str> = remaining
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(k, _)| *k)
            .collect();
        if ready.is_empty() {
            break;
        }
        for key in ready {
            remaining.remove(key);
            for deps in remaining.values_mut() {
                deps.remove(key);
            }
        }
    }
    if remaining.is_empty() {
        None
    } else {
        let mut stuck: Vec<String> = remaining.keys().map(|k| k.to_string()).collect();
        stuck.sort();
        Some(stuck)
    }
}

pub fn has_blocking(defects: &[Defect]) -> bool {
    defects.iter().any(|d| d.severity() == Severity::Blocking)
}

// ── Prompts ────────────────────────────────────────────────────────────────

pub fn plan_prompt(goal: &str, roster: &str) -> String {
    format!(
        "You manage a team working in this repository. Your goal:\n\n{goal}\n\n\
         Your specialists:\n{roster}\n\n\
         Tier tells you how much reasoning each person brings: easy is fast and cheap, best for \
         mechanical or well-specified edits; medium is ordinary engineering work; complex is deep \
         design and tricky reasoning, slowest and most expensive. \"Works like\" is how they \
         actually operate — match the assignment to the person, not the person to the assignment.\n\n\
         STEP 1 — LOOK BEFORE YOU PLAN.\n\
         Use Read, Grep and Glob to see what is actually here: the layout, the build files, the \
         existing conventions, whether any of this already exists. Do not plan against an imagined \
         repository. You will have to list what you opened.\n\n\
         STEP 2 — BREAK THE GOAL INTO SHIPPABLE UNITS.\n\
         Each assignment is one focused piece of work a specialist can finish and hand back for \
         someone else to build on — think fifteen to thirty minutes, not an afternoon. Rules:\n\
         - A specialist may hold several assignments. Sequencing the same person through three \
         small steps is BETTER than handing them one big one.\n\
         - Never write an assignment that says \"build X from scratch\", \"set up the whole Y\", or \
         lists more than about five things to do. If a brief needs the word \"and\" more than \
         twice, split it.\n\
         - The first assignment in any new area must be a thin end-to-end slice — a skeleton plus \
         one working path — so the next person has something concrete to extend. Breadth comes \
         after depth, never before.\n\
         - Order matters: use depends_on so schema lands before API, and API before UI.\n\
         - Aim for four to eight assignments. {MAX_TASKS} is the hard ceiling.\n\n\
         STEP 3 — WRITE EACH BRIEF SO IT NEEDS NO FOLLOW-UP QUESTION.\n\
         Name the files or directories to create or change. State the contracts other assignments \
         will rely on — route paths, function signatures, table names — so the next person doesn't \
         have to guess. Say explicitly what is OUT of scope because someone else has it.\n\
         Give one to {MAX_DONE_WHEN} done_when lines: checkable facts, not intentions.\n\
         \x20 Good: \"GET /api/leads returns a JSON list and pytest tests/test_leads.py passes\"\n\
         \x20 Bad:  \"the backend works well\"\n\
         Set size: \"small\" is one file or one function, \"medium\" is a few files in one layer, \
         \"large\" is a whole layer — allowed at most {MAX_LARGE_TASKS} time in the plan, and only \
         when splitting genuinely is not possible.\n\n\
         STEP 4 — SAY WHAT EACH ASSIGNMENT TOUCHES.\n\
         List the files or directories each one will create or change in `touches`. Assignments \
         with no dependency between them AND no overlap in `touches` are run at the same time, so \
         being accurate here is what makes the team fast. Be honest rather than optimistic: if two \
         assignments really do share a file, say so and they will be sequenced. Leave `touches` \
         empty only when you genuinely cannot predict the scope — that one will run alone.\n\n\
         Reply with ONLY a JSON object, no prose and no markdown fences:\n\
         {{\"summary\": \"one or two sentences: what you found in the repo, and how you are \
         splitting the work\",\n\
         \x20\"inspected\": [\"paths you actually opened or searched\"],\n\
         \x20\"tasks\": [{{\"key\": \"short_snake_case_id\",\n\
         \x20           \"title\": \"short imperative title\",\n\
         \x20           \"brief\": \"what to change, where, which contracts to honour, what is out of scope\",\n\
         \x20           \"assignee\": \"exact specialist name from the roster\",\n\
         \x20           \"done_when\": [\"checkable fact\", \"checkable fact\"],\n\
         \x20           \"size\": \"small|medium|large\",\n\
         \x20           \"depends_on\": [\"keys that must finish first\"],\n\
         \x20           \"touches\": [\"paths this will create or change\"]}}]}}"
    )
}

pub fn repair_prompt(defects: &[Defect]) -> String {
    let list = defects
        .iter()
        .map(|d| format!("- {}", d.message()))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Your plan has problems I can't run:\n\n{list}\n\n\
         Send the corrected plan — the WHOLE plan, not just the parts you changed. Same JSON \
         shape, no prose, no fences."
    )
}

/// The worker's brief, with its acceptance criteria and whatever context its
/// dependencies produced.
pub fn assignment_prompt(
    member_name: &str,
    member_title: &str,
    team_name: &str,
    goal: &str,
    title: &str,
    brief: &str,
    done_when: &[String],
    context: &str,
) -> String {
    let criteria = if done_when.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nDone when:\n{}",
            done_when
                .iter()
                .map(|d| format!("- {d}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    format!(
        "You are {member_name} ({member_title}) on {team_name}. The team's goal:\n\n{goal}\n\n\
         Your assignment — {title}:\n{brief}{criteria}{context}\n\n\
         Work in this repository. Post a short note with mcp__aichip__post_message when you start \
         and whenever you finish a meaningful chunk — the team and a human are watching, and ten \
         silent minutes reads as a hang. Use mcp__aichip__ask_manager if you hit a decision that \
         isn't yours to make.\n\
         Stay inside this assignment: if you spot work that belongs to someone else, say so in a \
         message rather than doing it yourself.\n\
         Finish with a short summary — what you changed, and which \"done when\" lines you can \
         honestly claim."
    )
}

/// Tier label for the roster listing.
pub fn tier_label(tier: ModelTier) -> &'static str {
    match tier {
        ModelTier::Easy => "easy",
        ModelTier::Medium => "medium",
        ModelTier::Complex => "complex",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::org::roster::Member;
    use aichip_shared::ModelTier;
    use uuid::Uuid;

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

    fn roster() -> Vec<Member> {
        vec![member("Priya"), member("Rex")]
    }

    fn task(key: &str, assignee: &str) -> PlannedTask {
        PlannedTask {
            key: key.into(),
            title: format!("Do {key}"),
            brief: "x".repeat(MIN_BRIEF_CHARS + 20),
            assignee: assignee.into(),
            done_when: vec!["tests pass".into()],
            size: TaskSize::Medium,
            depends_on: vec![],
            touches: vec![],
        }
    }

    fn good_plan() -> Plan {
        Plan {
            summary: "split in two".into(),
            inspected: vec!["src/".into()],
            tasks: vec![task("api", "Priya"), task("ui", "Rex")],
        }
    }

    #[test]
    fn a_clean_plan_has_no_defects() {
        assert_eq!(inspect_plan(&good_plan(), &roster()), vec![]);
    }

    /// The bug that used to kill whole runs: one absent field was a parse
    /// error rather than something the manager could fix.
    #[test]
    fn a_missing_field_is_repairable_not_unparseable() {
        let raw = r#"{"inspected":["src/"],"tasks":[{"key":"api","title":"Do api","assignee":"Priya","done_when":["ok"]}]}"#;
        let plan = parse_plan(raw).expect("missing brief must still parse");
        let defects = inspect_plan(&plan, &roster());
        assert!(defects.iter().any(|d| matches!(
            d,
            Defect::MissingField { field: "brief", .. }
        )));
        assert!(!defects.iter().any(|d| matches!(d, Defect::Unparseable(_))));
    }

    #[test]
    fn a_monolithic_brief_is_blocking() {
        let mut plan = good_plan();
        plan.tasks[0].brief = "x".repeat(MAX_BRIEF_CHARS + 1);
        let defects = inspect_plan(&plan, &roster());
        let brief_defect = defects
            .iter()
            .find(|d| matches!(d, Defect::BriefTooLong { .. }))
            .expect("oversize brief flagged");
        assert_eq!(brief_defect.severity(), Severity::Blocking);
        assert!(brief_defect.message().contains("split it"));
    }

    #[test]
    fn an_unknown_assignee_is_blocking_rather_than_silently_reassigned() {
        let mut plan = good_plan();
        plan.tasks[0].assignee = "Nobody".into();
        let defects = inspect_plan(&plan, &roster());
        assert!(defects
            .iter()
            .any(|d| matches!(d, Defect::UnknownAssignee { .. })));
        assert!(has_blocking(&defects));
    }

    #[test]
    fn assignee_matching_tolerates_case_and_padding() {
        let mut plan = good_plan();
        plan.tasks[0].assignee = "  priya ".into();
        assert!(!inspect_plan(&plan, &roster())
            .iter()
            .any(|d| matches!(d, Defect::UnknownAssignee { .. })));
    }

    #[test]
    fn an_idle_specialist_is_advisory_and_can_never_fail_a_run() {
        let mut plan = good_plan();
        plan.tasks[1].assignee = "Priya".into(); // Rex gets nothing
        let defects = inspect_plan(&plan, &roster());
        let idle = defects
            .iter()
            .find(|d| matches!(d, Defect::IdleSpecialist { .. }))
            .expect("idle specialist noticed");
        assert_eq!(idle.severity(), Severity::Advisory);
        assert!(!has_blocking(&defects), "advisories must not block");
    }

    #[test]
    fn one_person_hoarding_the_plan_is_flagged() {
        let plan = Plan {
            summary: "s".into(),
            inspected: vec!["src/".into()],
            tasks: vec![
                task("a", "Priya"),
                task("b", "Priya"),
                task("c", "Priya"),
                task("d", "Rex"),
            ],
        };
        assert!(inspect_plan(&plan, &roster())
            .iter()
            .any(|d| matches!(d, Defect::OverloadedSpecialist { .. })));
    }

    #[test]
    fn missing_and_duplicate_keys_are_caught() {
        let mut plan = good_plan();
        plan.tasks[1].key = "api".into();
        assert!(inspect_plan(&plan, &roster())
            .iter()
            .any(|d| matches!(d, Defect::DuplicateKey { .. })));
    }

    #[test]
    fn dependency_problems_are_caught() {
        let mut plan = good_plan();
        plan.tasks[0].depends_on = vec!["api".into()];
        assert!(inspect_plan(&plan, &roster())
            .iter()
            .any(|d| matches!(d, Defect::SelfDependency { .. })));

        let mut plan = good_plan();
        plan.tasks[0].depends_on = vec!["ghost".into()];
        assert!(inspect_plan(&plan, &roster())
            .iter()
            .any(|d| matches!(d, Defect::UnknownDependency { .. })));

        let mut plan = good_plan();
        plan.tasks[0].depends_on = vec!["ui".into()];
        plan.tasks[1].depends_on = vec!["api".into()];
        let cycle = inspect_plan(&plan, &roster());
        assert!(cycle.iter().any(|d| matches!(d, Defect::CyclicDependency { .. })));
    }

    #[test]
    fn empty_and_oversized_plans_are_caught() {
        assert_eq!(inspect_plan(&Plan::default(), &roster()), vec![Defect::NoTasks]);

        let mut plan = good_plan();
        plan.tasks = (0..MAX_TASKS + 1).map(|i| task(&format!("t{i}"), "Priya")).collect();
        assert!(inspect_plan(&plan, &roster())
            .iter()
            .any(|d| matches!(d, Defect::TooManyTasks { .. })));
    }

    #[test]
    fn more_than_one_large_task_is_blocking() {
        let mut plan = good_plan();
        plan.tasks[0].size = TaskSize::Large;
        plan.tasks[1].size = TaskSize::Large;
        assert!(inspect_plan(&plan, &roster())
            .iter()
            .any(|d| matches!(d, Defect::TooManyLarge { .. })));
    }

    #[test]
    fn planning_without_reading_is_advisory() {
        let mut plan = good_plan();
        plan.inspected.clear();
        let defects = inspect_plan(&plan, &roster());
        assert!(defects.iter().any(|d| matches!(d, Defect::NothingInspected)));
        assert!(!has_blocking(&defects));
    }

    /// A literal guard on the sentence that caused monolithic assignments.
    #[test]
    fn the_prompt_no_longer_rations_assignments_per_specialist() {
        let prompt = plan_prompt("build a thing", "- Priya (Backend) · tier: medium");
        assert!(!prompt.contains("at most one per specialist"));
        assert!(prompt.contains("A specialist may hold several assignments"));
        assert!(prompt.contains("build a thing"));
        assert!(prompt.contains("Priya"));
        assert!(prompt.contains("done_when"));
    }

    #[test]
    fn the_repair_prompt_carries_the_actual_defects() {
        let prompt = repair_prompt(&[
            Defect::BriefTooLong {
                key: "build_backend".into(),
                chars: 3140,
            },
            Defect::NoDoneWhen { key: "wire_ui".into() },
        ]);
        assert!(prompt.contains("build_backend"));
        assert!(prompt.contains("3140"));
        assert!(prompt.contains("wire_ui"));
        assert!(prompt.contains("WHOLE plan"));
    }

    #[test]
    fn the_assignment_prompt_states_the_acceptance_criteria() {
        let prompt = assignment_prompt(
            "Priya", "Backend", "Squad", "ship it", "Add API",
            "make the endpoint", &["tests pass".into()], "",
        );
        assert!(prompt.contains("Done when:\n- tests pass"));
        assert!(prompt.contains("post_message"));
        assert!(prompt.contains("honestly claim"));
    }
}
