//! Diffing two revisions of a page.
//!
//! Over **text**, never HTML. Two model passes that produce identical prose
//! emit different markup — attribute order, `<p>` versus `<div>`, stray
//! `&nbsp;` — so an HTML line diff reports a one-word fix as a total rewrite.
//! A reviewer who cannot read the diff stops reading it, and rubber-stamping
//! agent rewrites is the exact failure the revision log exists to prevent.
//!
//! The output is unified-diff format because the dashboard already has a
//! renderer for it: the same `annotateDiff` pipeline that draws code review.

use similar::{ChangeTag, TextDiff};

/// A unified diff between two block-per-line projections.
pub fn unified(from: &str, to: &str, from_label: &str, to_label: &str) -> String {
    let (from, to) = (terminate(from), terminate(to));
    let diff = TextDiff::from_lines(&from, &to);
    let mut out = format!("--- {from_label}\n+++ {to_label}\n");
    // Three lines of context: enough to place a change in its section, few
    // enough that a small edit doesn't render half the document.
    for (i, group) in diff.grouped_ops(3).iter().enumerate() {
        if i > 0 {
            out.push_str("@@\n");
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                out.push(sign);
                out.push_str(change.value().trim_end_matches('\n'));
                out.push('\n');
            }
        }
    }
    out
}

/// Give the text a trailing newline.
///
/// `from_lines` treats a final line without one as a different token from the
/// same line with one, so a page whose projection happens not to end in a
/// newline reports its last block as changed on every single save.
fn terminate(s: &str) -> String {
    if s.is_empty() || s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// How much changed, for a one-line summary above the diff.
pub struct Delta {
    pub added: usize,
    pub removed: usize,
}

pub fn delta(from: &str, to: &str) -> Delta {
    let (from, to) = (terminate(from), terminate(to));
    let diff = TextDiff::from_lines(&from, &to);
    let mut d = Delta { added: 0, removed: 0 };
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => d.added += 1,
            ChangeTag::Delete => d.removed += 1,
            ChangeTag::Equal => {}
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_produces_no_hunks() {
        let d = unified("a\nb\n", "a\nb\n", "1", "2");
        assert!(!d.contains("\n+a") && !d.contains("\n-a"), "{d}");
        assert_eq!(delta("a\nb", "a\nb").added, 0);
    }

    /// The property the whole review UI rests on: changing one block must
    /// report one block changed, not the whole page.
    #[test]
    fn a_one_line_change_touches_one_line() {
        let before = "Intro.\nStep one.\nStep two.\nOutro.";
        let after = "Intro.\nStep one, revised.\nStep two.\nOutro.";
        let d = delta(before, after);
        assert_eq!((d.added, d.removed), (1, 1), "{}", unified(before, after, "a", "b"));
    }

    #[test]
    fn the_output_is_the_format_the_dashboard_already_renders() {
        let d = unified("old\n", "new\n", "revision 1", "revision 2");
        assert!(d.starts_with("--- revision 1\n+++ revision 2\n"), "{d}");
        assert!(d.contains("-old"));
        assert!(d.contains("+new"));
    }

    #[test]
    fn additions_and_removals_are_counted_separately() {
        let d = delta("a\nb\nc", "a\nc\nd\ne");
        assert_eq!(d.removed, 1);
        assert_eq!(d.added, 2);
    }

    /// Whether the projection happens to end in a newline is an accident of
    /// the last block; it must not make the final line look edited.
    #[test]
    fn a_missing_trailing_newline_is_not_a_change() {
        assert_eq!(delta("a\nb", "a\nb\n").added, 0);
        assert_eq!(delta("a\nb\n", "a\nb").removed, 0);
    }

    #[test]
    fn an_empty_side_reads_as_wholly_added_or_removed() {
        assert_eq!(delta("", "a\nb").added, 2);
        assert_eq!(delta("a\nb", "").removed, 2);
    }
}
