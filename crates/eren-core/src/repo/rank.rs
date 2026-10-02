//! How much of the project leans on each file.
//!
//! PageRank over the import edges, power-iterated. Twenty-five lines rather
//! than a graph library because that is all it is, and because a dependency
//! whose numbers we cannot explain has no business deciding what a map
//! emphasises.
//!
//! **What this is for, and what it is not.** Measured on this repository the
//! top of the list is `api.ts` at 93 importers and `db.rs` at 27 — the
//! infrastructure everybody already knows about, and never the answer to
//! "where do I make this change". So rank drives node *size* and breaks ties;
//! it does not order search results and it does not choose what goes into a
//! prompt. Ranking by importance would reliably recommend the wrong files.

/// The share of a file's importance passed along its imports; the rest is
/// spread evenly. 0.85 is the original paper's, and the number is not load
/// bearing here — the ordering is stable across anything from 0.7 to 0.9.
const DAMPING: f32 = 0.85;

/// Power iteration converges geometrically; on a graph of a few hundred nodes
/// twenty rounds is far past the point where the ordering stops moving.
const ITERATIONS: usize = 20;

/// Importance per node, in the caller's node order, normalized so the largest
/// is exactly 1.0.
///
/// Normalizing to the maximum rather than to a sum is deliberate: the value is
/// consumed as "how big is this dot compared to the biggest dot", and a raw
/// PageRank score of 0.004 conveys nothing to the code that has to size it.
///
/// `edges` are `(from, to)` index pairs. Out-of-range indices are ignored
/// rather than panicking — an edge table is data, and data from a database is
/// not a proof.
pub fn pagerank(node_count: usize, edges: &[(usize, usize)]) -> Vec<f32> {
    if node_count == 0 {
        return vec![];
    }
    let n = node_count as f32;

    let mut out_degree = vec![0usize; node_count];
    let mut incoming: Vec<Vec<usize>> = vec![vec![]; node_count];
    for &(from, to) in edges {
        if from >= node_count || to >= node_count || from == to {
            continue; // a file importing itself tells us nothing
        }
        out_degree[from] += 1;
        incoming[to].push(from);
    }

    let mut score = vec![1.0f32 / n; node_count];
    for _ in 0..ITERATIONS {
        // A file that imports nothing would otherwise leak its share out of
        // the graph on every round, and every score would decay towards zero
        // together. Collected and redistributed evenly, which is the standard
        // treatment and the reason a leaf's rank stays meaningful.
        let dangling: f32 = (0..node_count)
            .filter(|&i| out_degree[i] == 0)
            .map(|i| score[i])
            .sum();

        let mut next = vec![(1.0 - DAMPING) / n + DAMPING * dangling / n; node_count];
        for to in 0..node_count {
            let mut sum = 0.0;
            for &from in &incoming[to] {
                sum += score[from] / out_degree[from] as f32;
            }
            next[to] += DAMPING * sum;
        }
        score = next;
    }

    let max = score.iter().cloned().fold(0.0f32, f32::max);
    if max <= 0.0 {
        return vec![0.0; node_count];
    }
    score.iter().map(|s| s / max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_imported_file_ranks_highest() {
        // 0,1,2 all import 3; 3 imports nothing.
        let r = pagerank(4, &[(0, 3), (1, 3), (2, 3)]);
        assert_eq!(r[3], 1.0);
        assert!(r[0] < r[3] && r[1] < r[3] && r[2] < r[3]);
        // The three importers are indistinguishable and must score equally —
        // otherwise the layout would order them by array position, which is a
        // fact about the database and not about the code.
        assert!((r[0] - r[1]).abs() < 1e-6 && (r[1] - r[2]).abs() < 1e-6);
    }

    #[test]
    fn importance_flows_through_an_importer_not_just_from_it() {
        // A hub imported by many, importing one thing: that one thing should
        // outrank a file imported by a single leaf.
        let edges = [(0, 4), (1, 4), (2, 4), (3, 4), (4, 5), (0, 6)];
        let r = pagerank(7, &edges);
        assert!(r[5] > r[6], "inherited importance {} vs {}", r[5], r[6]);
    }

    #[test]
    fn a_cycle_terminates_and_shares_evenly() {
        let r = pagerank(3, &[(0, 1), (1, 2), (2, 0)]);
        assert!(r.iter().all(|s| s.is_finite()));
        assert!((r[0] - r[1]).abs() < 1e-5 && (r[1] - r[2]).abs() < 1e-5);
    }

    #[test]
    fn a_graph_with_no_edges_ranks_everything_the_same() {
        let r = pagerank(5, &[]);
        assert_eq!(r.len(), 5);
        assert!(r.iter().all(|s| (*s - 1.0).abs() < 1e-6));
    }

    #[test]
    fn nothing_and_nonsense_are_survivable() {
        assert!(pagerank(0, &[(0, 1)]).is_empty());
        // Indices past the end are data, not a proof; they are dropped.
        let r = pagerank(2, &[(0, 99), (99, 0), (1, 1)]);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn scores_are_stable_between_runs_on_the_same_input() {
        let edges = [(0, 1), (1, 2), (2, 3), (3, 1), (0, 3), (4, 1)];
        assert_eq!(pagerank(5, &edges), pagerank(5, &edges));
    }
}
