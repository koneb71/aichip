//! Deciding what can run at the same time.
//!
//! Assignments share one worktree, so concurrency is safe exactly when the
//! work doesn't overlap: no dependency between them, and no shared files.
//! Both facts come from the manager's plan — `depends_on` and `touches` —
//! and both are checked here rather than trusted at execution time.
//!
//! The bias is deliberately conservative. A missed opportunity costs time;
//! a wrong guess means two agents writing the same file and the last one
//! winning silently, which is far worse than being slow.

use super::Assignment;
use std::collections::HashSet;

/// Ceiling on one batch. Every extra concurrent agent is another draw
/// against the same subscription rate limit, so this stays small.
pub const MAX_PARALLEL_ASSIGNMENTS: usize = 3;

/// Indices of the assignments whose dependencies are all satisfied.
///
/// Never returns empty for a non-empty input. If every remaining assignment
/// is waiting on another — a dependency cycle — the first one is released
/// anyway. The caller loops until nothing is pending, so "nothing is ready
/// but work remains" would spin forever; a confused plan should still run.
pub fn ready_now(pending: &[Assignment], satisfied: &HashSet<String>) -> Vec<usize> {
    let known: HashSet<&str> = pending.iter().map(|a| a.key.as_str()).collect();
    let ready: Vec<usize> = (0..pending.len())
        .filter(|i| {
            pending[*i].depends_on.iter().all(|dep| {
                let dep = dep.trim();
                // Unknown keys are ignored rather than blocking forever: a
                // hallucinated dependency shouldn't strand real work.
                satisfied.contains(dep) || !known.contains(dep)
            })
        })
        .collect();
    if ready.is_empty() && !pending.is_empty() {
        return vec![0];
    }
    ready
}

/// The assignments to start right now.
///
/// Always returns at least one when anything is ready. Additional ones join
/// only when their file scope is disjoint from everything already in the
/// batch — and an assignment that declared no scope runs by itself, because
/// "unknown" has to mean "might touch anything".
pub fn parallel_batch(
    pending: &[Assignment],
    satisfied: &HashSet<String>,
    max: usize,
) -> Vec<usize> {
    let ready = ready_now(pending, satisfied);
    let Some(&first) = ready.first() else {
        return vec![];
    };
    let mut batch = vec![first];
    if pending[first].touches.is_empty() || max <= 1 {
        return batch;
    }

    for &candidate in ready.iter().skip(1) {
        if batch.len() >= max {
            break;
        }
        let scope = &pending[candidate].touches;
        if scope.is_empty() {
            continue;
        }
        let clashes = batch
            .iter()
            .any(|&i| scopes_overlap(&pending[i].touches, scope));
        if !clashes {
            batch.push(candidate);
        }
    }
    batch
}

/// Do two declared file scopes share anything?
pub fn scopes_overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|x| b.iter().any(|y| paths_overlap(x, y)))
}

/// True when two paths are the same file, or one contains the other.
/// `backend/` and `backend/app/db.py` overlap; `backend/` and `frontend/`
/// do not.
fn paths_overlap(a: &str, b: &str) -> bool {
    let a = normalize(a);
    let b = normalize(b);
    if a.is_empty() || b.is_empty() {
        // An empty or root-ish path can't be reasoned about; assume the worst.
        return true;
    }
    a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// Strip the decorations a model reasonably writes — leading `./`, trailing
/// slashes, and glob tails — down to a comparable path.
fn normalize(path: &str) -> String {
    let mut p = path.trim().replace('\\', "/");
    for prefix in ["./", "/"] {
        while let Some(rest) = p.strip_prefix(prefix) {
            p = rest.to_string();
        }
    }
    // "src/**/*.rs" and "src/*" both mean "somewhere under src".
    if let Some(star) = p.find('*') {
        p.truncate(star);
    }
    p.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::org::plan::TaskSize;
    use uuid::Uuid;

    fn assignment(key: &str, depends_on: &[&str], touches: &[&str]) -> Assignment {
        Assignment {
            step_id: Uuid::new_v4(),
            key: key.into(),
            title: key.into(),
            brief: String::new(),
            assignee: "Rex".into(),
            done_when: vec![],
            size: TaskSize::Medium,
            depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
            touches: touches.iter().map(|s| s.to_string()).collect(),
            status: "queued".into(),
            output: None,
            attempt: 1,
        }
    }

    fn batch(pending: &[Assignment], satisfied: &[&str]) -> Vec<String> {
        let satisfied: HashSet<String> = satisfied.iter().map(|s| s.to_string()).collect();
        parallel_batch(pending, &satisfied, MAX_PARALLEL_ASSIGNMENTS)
            .into_iter()
            .map(|i| pending[i].key.clone())
            .collect()
    }

    #[test]
    fn disjoint_work_runs_together() {
        let pending = vec![
            assignment("api", &[], &["backend/app/routes.py"]),
            assignment("ui", &[], &["frontend/src/App.tsx"]),
        ];
        assert_eq!(batch(&pending, &[]), ["api", "ui"]);
    }

    #[test]
    fn work_sharing_a_file_is_sequenced() {
        let pending = vec![
            assignment("routes", &[], &["backend/app/routes.py"]),
            assignment("more_routes", &[], &["backend/app/routes.py"]),
        ];
        assert_eq!(batch(&pending, &[]), ["routes"]);
    }

    #[test]
    fn a_directory_contains_the_files_under_it() {
        let pending = vec![
            assignment("backend", &[], &["backend/"]),
            assignment("models", &[], &["backend/app/models.py"]),
        ];
        assert_eq!(
            batch(&pending, &[]),
            ["backend"],
            "nested scope is an overlap"
        );
    }

    #[test]
    fn an_undeclared_scope_runs_alone() {
        let pending = vec![
            assignment("mystery", &[], &[]),
            assignment("ui", &[], &["frontend/"]),
        ];
        assert_eq!(
            batch(&pending, &[]),
            ["mystery"],
            "unknown means it could touch anything"
        );
    }

    #[test]
    fn an_undeclared_scope_never_joins_someone_elses_batch() {
        let pending = vec![
            assignment("api", &[], &["backend/"]),
            assignment("mystery", &[], &[]),
            assignment("ui", &[], &["frontend/"]),
        ];
        assert_eq!(
            batch(&pending, &[]),
            ["api", "ui"],
            "mystery waits its turn"
        );
    }

    #[test]
    fn dependencies_still_gate_everything() {
        let both = vec![
            assignment("ui", &["api"], &["frontend/"]),
            assignment("api", &[], &["backend/"]),
        ];
        assert_eq!(batch(&both, &[]), ["api"], "ui is not ready yet");

        // Once api lands it leaves the pending set — a completed assignment
        // is never a candidate again.
        let after = vec![assignment("ui", &["api"], &["frontend/"])];
        assert_eq!(batch(&after, &["api"]), ["ui"]);
    }

    #[test]
    fn a_batch_is_capped() {
        let pending: Vec<Assignment> = (0..6)
            .map(|i| {
                let dir = format!("area{i}/");
                assignment(&format!("t{i}"), &[], &[dir.as_str()])
            })
            .collect();
        assert_eq!(batch(&pending, &[]).len(), MAX_PARALLEL_ASSIGNMENTS);
    }

    #[test]
    fn nothing_ready_is_an_empty_batch() {
        assert!(batch(&[], &[]).is_empty());
    }

    /// The executor loops until nothing is pending, so a batch that comes
    /// back empty with work still queued is an infinite loop. A cycle has to
    /// be broken here rather than deadlock.
    #[test]
    fn a_cyclic_plan_still_makes_progress() {
        let pending = vec![
            assignment("a", &["b"], &["src/a"]),
            assignment("b", &["a"], &["src/b"]),
        ];
        assert_eq!(batch(&pending, &[]), ["a"]);
    }

    #[test]
    fn a_dependency_on_dropped_work_does_not_block() {
        // The caller passes `skipped` keys in as satisfied.
        let pending = vec![assignment("ui", &["gone"], &["frontend/"])];
        assert_eq!(batch(&pending, &["gone"]), ["ui"]);
    }

    #[test]
    fn a_hallucinated_dependency_does_not_strand_work() {
        let pending = vec![assignment("a", &["ghost"], &["src/"])];
        assert_eq!(batch(&pending, &[]), ["a"]);
    }

    #[test]
    fn paths_are_compared_past_the_decorations_a_model_writes() {
        assert!(paths_overlap("./backend/app", "backend/app/"));
        assert!(paths_overlap("src/**/*.rs", "src/main.rs"));
        assert!(paths_overlap("/backend", "backend/db.py"));
        assert!(!paths_overlap("backend/", "frontend/"));
        // A near-miss that is genuinely distinct.
        assert!(!paths_overlap("app/models.py", "app/models_test.py"));
    }

    #[test]
    fn an_unreasonable_path_is_treated_as_touching_everything() {
        assert!(paths_overlap("/", "anything/at/all"));
        assert!(paths_overlap("*", "src/main.rs"));
    }
}
