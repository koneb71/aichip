//! Workflow definitions: the YAML users write in `.aichip/workflows/*.yaml`,
//! plus validation, dependency ordering, and prompt interpolation.
//!
//! One format covers every pattern: a plain sequence is a pipeline, a step
//! with `strategy.parallel` fans out, and a step that `needs` a fan-out step
//! sees all of its outputs (that's a debate with a judge).

use crate::ModelTier;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub on: Option<Trigger>,
    #[serde(default)]
    pub defaults: Defaults,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trigger {
    /// Cron expression; picked up by the scheduler.
    #[serde(default)]
    pub schedule: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_engine")]
    pub engine: String,
    /// Tier name ("easy"/"medium"/"complex") or a literal model id.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, alias = "permissionMode")]
    pub permission_mode: Option<String>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            engine: default_engine(),
            model: None,
            permission_mode: None,
        }
    }
}

fn default_engine() -> String {
    "claude-code".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    #[serde(default)]
    pub needs: Vec<String>,
    pub prompt: String,
    /// Name of an agent in the workspace library; supplies system prompt,
    /// tier, and tools unless overridden here.
    #[serde(default)]
    pub agent: Option<String>,
    /// Tier name or literal model id; overrides the agent's tier.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub session: SessionMode,
    #[serde(default)]
    pub strategy: Option<Strategy>,
}

impl Step {
    pub fn parallelism(&self) -> usize {
        self.strategy.as_ref().map(|s| s.parallel).unwrap_or(1).max(1)
    }

    pub fn isolated_worktrees(&self) -> bool {
        self.strategy
            .as_ref()
            .map(|s| s.isolated_worktrees)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    /// Start a new engine session for this step.
    #[default]
    Fresh,
    /// Resume the session of the step this one depends on, so the agent
    /// keeps its context from the previous stage.
    Continue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    /// Run this step N times concurrently (independent attempts).
    #[serde(default = "one")]
    pub parallel: usize,
    /// Give each attempt its own git worktree so they can't collide.
    #[serde(default, alias = "isolatedWorktrees")]
    pub isolated_worktrees: bool,
}

fn one() -> usize {
    1
}

pub const MAX_PARALLEL: usize = 8;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WorkflowError {
    #[error("workflow name is required")]
    MissingName,
    #[error("workflow must have at least one step")]
    NoSteps,
    #[error("duplicate step id: {0}")]
    DuplicateStep(String),
    #[error("step id must be alphanumeric with - or _: {0}")]
    InvalidStepId(String),
    #[error("step '{step}' needs unknown step '{missing}'")]
    UnknownDependency { step: String, missing: String },
    #[error("step '{0}' has an empty prompt")]
    EmptyPrompt(String),
    #[error("step '{step}' parallel must be 1..={max}")]
    BadParallelism { step: String, max: usize },
    #[error("steps form a cycle: {0}")]
    Cycle(String),
}

impl Workflow {
    pub fn from_yaml(yaml: &str) -> anyhow::Result<Self> {
        let workflow: Workflow = serde_yaml::from_str(yaml)?;
        workflow.validate()?;
        Ok(workflow)
    }

    pub fn validate(&self) -> Result<(), WorkflowError> {
        if self.name.trim().is_empty() {
            return Err(WorkflowError::MissingName);
        }
        if self.steps.is_empty() {
            return Err(WorkflowError::NoSteps);
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for step in &self.steps {
            if !seen.insert(step.id.as_str()) {
                return Err(WorkflowError::DuplicateStep(step.id.clone()));
            }
            if step.id.is_empty()
                || !step
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(WorkflowError::InvalidStepId(step.id.clone()));
            }
            if step.prompt.trim().is_empty() {
                return Err(WorkflowError::EmptyPrompt(step.id.clone()));
            }
            let parallel = step.parallelism();
            if parallel > MAX_PARALLEL {
                return Err(WorkflowError::BadParallelism {
                    step: step.id.clone(),
                    max: MAX_PARALLEL,
                });
            }
        }
        for step in &self.steps {
            for need in &step.needs {
                if !seen.contains(need.as_str()) {
                    return Err(WorkflowError::UnknownDependency {
                        step: step.id.clone(),
                        missing: need.clone(),
                    });
                }
            }
        }
        self.layers().map(|_| ())
    }

    pub fn step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.id == id)
    }

    /// Dependency-ordered batches (Kahn's algorithm). Every step in a batch
    /// can run concurrently; the executor waits for a batch before the next.
    pub fn layers(&self) -> Result<Vec<Vec<String>>, WorkflowError> {
        let mut remaining: HashMap<&str, HashSet<&str>> = self
            .steps
            .iter()
            .map(|s| {
                (
                    s.id.as_str(),
                    s.needs.iter().map(String::as_str).collect::<HashSet<_>>(),
                )
            })
            .collect();
        let mut layers: Vec<Vec<String>> = vec![];
        let mut done: HashSet<&str> = HashSet::new();

        while !remaining.is_empty() {
            // Preserve authoring order within a layer for stable display.
            let ready: Vec<&str> = self
                .steps
                .iter()
                .map(|s| s.id.as_str())
                .filter(|id| {
                    remaining
                        .get(id)
                        .is_some_and(|needs| needs.iter().all(|n| done.contains(n)))
                })
                .collect();
            if ready.is_empty() {
                let mut stuck: Vec<&str> = remaining.keys().copied().collect();
                stuck.sort();
                return Err(WorkflowError::Cycle(stuck.join(", ")));
            }
            for id in &ready {
                remaining.remove(id);
            }
            done.extend(ready.iter().copied());
            layers.push(ready.into_iter().map(String::from).collect());
        }
        Ok(layers)
    }

    /// Resolve a step's model: an explicit tier name maps through the user's
    /// tier table; anything else is treated as a literal model id.
    pub fn resolve_model(
        &self,
        step: &Step,
        agent_tier: Option<ModelTier>,
        tiers: &crate::TierMapping,
    ) -> String {
        let spec = step.model.clone().or_else(|| self.defaults.model.clone());
        match spec {
            Some(s) => match parse_tier(&s) {
                Some(tier) => tiers.model_for(tier).to_string(),
                None => s,
            },
            None => tiers
                .model_for(agent_tier.unwrap_or_default())
                .to_string(),
        }
    }
}

/// Build a runnable workflow from a team: the coordination pattern decides
/// the shape, the member agents supply the roles.
///
/// - `pipeline` — members run in order, each seeing the previous output.
/// - `debate` — every member but the last attempts independently in its own
///   worktree; the last member judges all attempts.
/// - `swarm` — members work in parallel on the same goal, no judge.
pub fn from_team(team_name: &str, pattern: &str, members: &[String], goal: &str) -> Workflow {
    let mut steps: Vec<Step> = vec![];
    let step_id = |i: usize, name: &str| {
        let slug: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("s{}_{}", i + 1, slug.trim_matches('_'))
    };

    match pattern {
        "debate" if members.len() >= 2 => {
            let (solvers, judge) = members.split_at(members.len() - 1);
            for (i, agent) in solvers.iter().enumerate() {
                steps.push(Step {
                    id: step_id(i, agent),
                    needs: vec![],
                    prompt: format!("{goal}\n\nWork independently — other agents are attempting this in parallel. Produce your best solution and end with a short summary of your approach."),
                    agent: Some(agent.clone()),
                    model: None,
                    session: SessionMode::Fresh,
                    strategy: Some(Strategy {
                        parallel: 1,
                        isolated_worktrees: true,
                    }),
                });
            }
            let refs = solvers
                .iter()
                .enumerate()
                .map(|(i, a)| format!("--- {a} ---\n{{{{ steps.{}.output }}}}", step_id(i, a)))
                .collect::<Vec<_>>()
                .join("\n\n");
            steps.push(Step {
                id: step_id(members.len() - 1, &judge[0]),
                needs: solvers
                    .iter()
                    .enumerate()
                    .map(|(i, a)| step_id(i, a))
                    .collect(),
                prompt: format!(
                    "Goal: {goal}\n\nHere are the independent attempts:\n\n{refs}\n\nJudge them, pick the strongest, and explain the decision. Graft in the best ideas from the others where they help."
                ),
                agent: Some(judge[0].clone()),
                model: None,
                session: SessionMode::Fresh,
                strategy: None,
            });
        }
        "swarm" => {
            for (i, agent) in members.iter().enumerate() {
                steps.push(Step {
                    id: step_id(i, agent),
                    needs: vec![],
                    prompt: format!("{goal}\n\nHandle the part of this that fits your role. Other agents are working on it in parallel."),
                    agent: Some(agent.clone()),
                    model: None,
                    session: SessionMode::Fresh,
                    strategy: None,
                });
            }
        }
        // "pipeline" and anything unrecognized.
        _ => {
            for (i, agent) in members.iter().enumerate() {
                let previous = i.checked_sub(1).map(|p| step_id(p, &members[p]));
                let prompt = match &previous {
                    Some(prev) => format!(
                        "{goal}\n\nThe previous stage produced:\n{{{{ steps.{prev}.output }}}}\n\nContinue from there in your role."
                    ),
                    None => goal.to_string(),
                };
                steps.push(Step {
                    id: step_id(i, agent),
                    needs: previous.into_iter().collect(),
                    prompt,
                    agent: Some(agent.clone()),
                    model: None,
                    session: SessionMode::Fresh,
                    strategy: None,
                });
            }
        }
    }

    Workflow {
        name: team_name.to_string(),
        description: format!("{pattern} run of team {team_name}"),
        on: None,
        defaults: Defaults::default(),
        steps,
    }
}

pub fn parse_tier(s: &str) -> Option<ModelTier> {
    match s.to_ascii_lowercase().as_str() {
        "easy" => Some(ModelTier::Easy),
        "medium" => Some(ModelTier::Medium),
        "complex" => Some(ModelTier::Complex),
        _ => None,
    }
}

/// Outputs of completed steps, keyed by step id. A fan-out step has several.
pub type StepOutputs = HashMap<String, Vec<String>>;

/// Replace `{{ steps.<id>.output }}` (first output) and
/// `{{ steps.<id>.outputs }}` (all, numbered) in a prompt. Unknown or
/// not-yet-run references collapse to an empty string rather than failing —
/// a half-substituted prompt is more useful than a dead run.
pub fn interpolate(template: &str, outputs: &StepOutputs) -> String {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let Some(end_rel) = rest[start..].find("}}") else { break };
        let end = start + end_rel;
        result.push_str(&rest[..start]);
        let expr = rest[start + 2..end].trim();
        result.push_str(&resolve_expr(expr, outputs));
        rest = &rest[end + 2..];
    }
    result.push_str(rest);
    result
}

fn resolve_expr(expr: &str, outputs: &StepOutputs) -> String {
    let parts: Vec<&str> = expr.split('.').map(str::trim).collect();
    match parts.as_slice() {
        ["steps", id, "output"] => outputs
            .get(*id)
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or_default(),
        ["steps", id, "outputs"] => outputs
            .get(*id)
            .map(|v| {
                v.iter()
                    .enumerate()
                    .map(|(i, o)| format!("--- attempt {} ---\n{o}", i + 1))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PIPELINE: &str = r#"
name: fix-hard-bug
defaults:
  engine: claude-code
  permissionMode: auto_edit
steps:
  - id: triage
    model: easy
    prompt: "Reproduce issue #42 and summarize the root cause."
  - id: attempt
    needs: [triage]
    model: medium
    strategy: { parallel: 3, isolated_worktrees: true }
    prompt: "Fix the bug: {{ steps.triage.output }}"
  - id: judge
    needs: [attempt]
    model: complex
    prompt: "Pick the best fix:\n{{ steps.attempt.outputs }}"
"#;

    #[test]
    fn parses_and_orders_a_debate_pipeline() {
        let wf = Workflow::from_yaml(PIPELINE).unwrap();
        assert_eq!(wf.name, "fix-hard-bug");
        assert_eq!(wf.steps.len(), 3);
        assert_eq!(wf.step("attempt").unwrap().parallelism(), 3);
        assert!(wf.step("attempt").unwrap().isolated_worktrees());
        assert_eq!(
            wf.layers().unwrap(),
            vec![vec!["triage"], vec!["attempt"], vec!["judge"]]
        );
    }

    #[test]
    fn independent_steps_share_a_layer() {
        let yaml = r#"
name: sweep
steps:
  - id: lint
    prompt: "lint it"
  - id: types
    prompt: "typecheck it"
  - id: report
    needs: [lint, types]
    prompt: "summarize"
"#;
        let wf = Workflow::from_yaml(yaml).unwrap();
        let layers = wf.layers().unwrap();
        assert_eq!(layers[0], vec!["lint", "types"]);
        assert_eq!(layers[1], vec!["report"]);
    }

    #[test]
    fn rejects_cycles_and_bad_references() {
        let cyclic = r#"
name: loop
steps:
  - id: a
    needs: [b]
    prompt: "x"
  - id: b
    needs: [a]
    prompt: "y"
"#;
        assert!(matches!(
            Workflow::from_yaml(cyclic).unwrap_err().downcast::<WorkflowError>(),
            Ok(WorkflowError::Cycle(_))
        ));

        let dangling = r#"
name: oops
steps:
  - id: a
    needs: [ghost]
    prompt: "x"
"#;
        assert!(Workflow::from_yaml(dangling).is_err());
    }

    #[test]
    fn rejects_duplicates_empty_prompts_and_overparallelism() {
        let dup = "name: d\nsteps:\n  - id: a\n    prompt: x\n  - id: a\n    prompt: y\n";
        assert!(Workflow::from_yaml(dup).is_err());

        let empty = "name: e\nsteps:\n  - id: a\n    prompt: '  '\n";
        assert!(Workflow::from_yaml(empty).is_err());

        let greedy = "name: g\nsteps:\n  - id: a\n    prompt: x\n    strategy: { parallel: 99 }\n";
        assert!(Workflow::from_yaml(greedy).is_err());
    }

    #[test]
    fn interpolates_single_and_fanned_out_outputs() {
        let mut outputs = StepOutputs::new();
        outputs.insert("triage".into(), vec!["null deref in auth.rs".into()]);
        outputs.insert("attempt".into(), vec!["fix A".into(), "fix B".into()]);

        assert_eq!(
            interpolate("Fix: {{ steps.triage.output }}", &outputs),
            "Fix: null deref in auth.rs"
        );
        let judged = interpolate("Pick:\n{{ steps.attempt.outputs }}", &outputs);
        assert!(judged.contains("--- attempt 1 ---\nfix A"));
        assert!(judged.contains("--- attempt 2 ---\nfix B"));
    }

    #[test]
    fn unknown_references_collapse_instead_of_breaking() {
        let outputs = StepOutputs::new();
        assert_eq!(interpolate("a{{ steps.ghost.output }}b", &outputs), "ab");
        assert_eq!(interpolate("a{{ nonsense }}b", &outputs), "ab");
        // An unterminated brace is left alone rather than eating the prompt.
        assert_eq!(interpolate("keep {{ this", &outputs), "keep {{ this");
    }

    #[test]
    fn team_patterns_produce_valid_runnable_workflows() {
        let members = vec!["Planner".to_string(), "Builder".to_string(), "Judge".to_string()];

        let pipeline = from_team("ship-it", "pipeline", &members, "Add dark mode");
        pipeline.validate().unwrap();
        assert_eq!(pipeline.layers().unwrap().len(), 3, "pipeline is sequential");
        assert!(pipeline.steps[1].prompt.contains("{{ steps.s1_planner.output }}"));
        assert_eq!(pipeline.steps[2].agent.as_deref(), Some("Judge"));

        let debate = from_team("bake-off", "debate", &members, "Fix the flaky test");
        debate.validate().unwrap();
        let layers = debate.layers().unwrap();
        assert_eq!(layers[0].len(), 2, "solvers attempt in parallel");
        assert_eq!(layers[1].len(), 1, "judge waits for all attempts");
        assert!(debate.steps[0].isolated_worktrees());
        assert!(debate.steps[2].prompt.contains("{{ steps.s1_planner.output }}"));
        assert!(debate.steps[2].prompt.contains("{{ steps.s2_builder.output }}"));

        let swarm = from_team("sweep", "swarm", &members, "Update deps");
        swarm.validate().unwrap();
        assert_eq!(swarm.layers().unwrap().len(), 1, "swarm is one wide layer");
    }

    #[test]
    fn one_member_debate_degrades_to_a_pipeline() {
        let solo = vec!["Solo".to_string()];
        let wf = from_team("t", "debate", &solo, "do it");
        wf.validate().unwrap();
        assert_eq!(wf.steps.len(), 1);
    }

    #[test]
    fn resolves_tier_names_and_literal_model_ids() {
        let wf = Workflow::from_yaml(PIPELINE).unwrap();
        let tiers = crate::TierMapping::default();
        assert_eq!(
            wf.resolve_model(wf.step("triage").unwrap(), None, &tiers),
            "claude-sonnet-5"
        );
        assert_eq!(
            wf.resolve_model(wf.step("judge").unwrap(), None, &tiers),
            "claude-fable-5"
        );

        let literal = "name: l\nsteps:\n  - id: a\n    prompt: x\n    model: some-custom-model\n";
        let wf = Workflow::from_yaml(literal).unwrap();
        assert_eq!(
            wf.resolve_model(wf.step("a").unwrap(), None, &tiers),
            "some-custom-model"
        );
    }

    #[test]
    fn step_without_model_falls_back_to_its_agent_tier() {
        let yaml = "name: a\nsteps:\n  - id: review\n    prompt: x\n    agent: Reviewer\n";
        let wf = Workflow::from_yaml(yaml).unwrap();
        let tiers = crate::TierMapping::default();
        assert_eq!(
            wf.resolve_model(wf.step("review").unwrap(), Some(ModelTier::Complex), &tiers),
            "claude-fable-5"
        );
    }
}
