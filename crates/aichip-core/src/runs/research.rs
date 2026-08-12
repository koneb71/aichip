//! Deep research: investigate a question about a project, produce a cited
//! report.
//!
//! The run is read-only over the project's real checkout — no worktree, no
//! branch, exactly the shape knowledge-base generation has — plus the CLI's
//! own `WebSearch` and `WebFetch`, which is the half no other run type gets.
//! What comes back is a markdown report that lives on the Research page and
//! can be filed into the knowledge base with one click.
//!
//! Everything here is pure, for the same reason `kb::write` and `task_plan`
//! are: the prompt's promises — cite your sources, repo before web, markdown
//! only — are asserted directly rather than discovered on a paid run.

use std::borrow::Cow;

/// Tools a research run may use: the read-only three, plus the web.
///
/// Claude Code takes these names verbatim; OpenCode maps `WebSearch` and
/// `WebFetch` to its own `websearch`/`webfetch` keys in the adapter.
pub const TOOLS: &[&str] = &["Read", "Grep", "Glob", "WebSearch", "WebFetch"];

/// What a research run must never do, whatever else is granted.
///
/// The allow-list above only pre-approves — denial is the part that binds
/// (the `task_plan::PLANNING_DENIED` war story). The five mutating tools are
/// non-negotiable because the run works in the user's real checkout. `Task`
/// is denied deliberately: research runs at the Complex tier, and a subagent
/// fan-out multiplies that spend invisibly while making the live transcript
/// on the page unreadable.
///
/// Note what is *not* here: `WebFetch`. `PLANNING_DENIED` denies it because a
/// plan must not act; here the web is the point.
pub const DENIED: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit", "Bash", "Task"];

/// How much of a question reaches the prompt. The framing around it is what
/// makes the reply usable, and a question long enough to bury the framing
/// has defeated it — the same budget every quoted input in this codebase has.
pub const QUESTION_MAX: usize = 4000;

fn clip(text: &str, max: usize) -> Cow<'_, str> {
    if text.chars().count() <= max {
        return Cow::Borrowed(text);
    }
    let cut: String = text.chars().take(max).collect();
    Cow::Owned(cut + "\n…")
}

/// The research brief.
///
/// `context` is the project's standing context, already fenced, or empty. It
/// goes after the question and **before** the format contract — the same
/// placement `kb::write::prompt` and `org::plan_prompt` settled on, because
/// this prompt also ends with an output contract and four thousand characters
/// after "reply with Markdown only" is how you get prose where the report
/// should start.
pub fn prompt(question: &str, context: &str) -> String {
    let question = clip(question.trim(), QUESTION_MAX);
    format!(
        "Research this question about the repository you are in:\n\n\
         {question}{context}\n\n\
         ## How to work\n\n\
         Investigate the repository first — Read, Grep and Glob are yours. \
         Ground every claim about this project in what is actually here: name \
         real files, real functions, real configuration. Then use WebSearch \
         and WebFetch for anything the repository cannot answer — library \
         documentation, release history, ecosystem context, prior art.\n\n\
         Keep the two apart as you write: say plainly which findings come \
         from this repository and which come from the web. If the two \
         disagree, the repository is what is true of this project.\n\n\
         If part of the question cannot be answered — the information is not \
         in the repo and not findable — say so in the report rather than \
         writing the version that is usually true. A visible gap is worth \
         more than a plausible invention.\n\n\
         ## Citations\n\n\
         Every claim sourced from the web carries its source as a markdown \
         link, inline where the claim is made. Every claim about the \
         repository names the file (and function, where it helps) it came \
         from.\n\n\
         ## Format\n\n\
         Reply with **Markdown only** — no preamble like \"Here is the \
         report\", and no code fence wrapped around the whole reply. Open \
         with a single `# ` heading naming the report, then the report \
         itself: sections, lists and tables where they genuinely read better \
         than prose, code blocks for code. End with a short **Sources** \
         section listing the web sources you actually used."
    )
}

/// Pull the report out of whatever the agent replied with.
///
/// The markdown analog of `kb::write::extract_html`: models wrap a whole
/// reply in a fence or introduce it with a sentence often enough that
/// trusting the raw reply would put ```markdown into a rendered page.
pub fn extract_report(reply: &str) -> String {
    let text = reply.trim();

    // A fence around everything wins if there is one: it is an explicit
    // statement of where the content starts and stops. Only when it wraps
    // the whole reply, though — a report legitimately contains fenced code
    // blocks, and grabbing the first one would eat the report.
    if text.starts_with("```") {
        let after = &text[3..];
        let body = after.split_once('\n').map(|(_, b)| b).unwrap_or(after);
        if let Some((inner, rest)) = body.rsplit_once("```") {
            if rest.trim().is_empty() {
                return inner.trim().to_string();
            }
        }
    }

    // Otherwise drop any prose before the first heading — the format contract
    // says the report opens with one.
    match text.find("# ") {
        Some(i) if text[..i].lines().count() <= 3 && !text[..i].contains("\n\n#") => {
            // Only strip a short preamble; a "# " deep inside the text is a
            // section of a report that (against instructions) has no title.
            let start = text[..i].rfind('\n').map(|n| n + 1).unwrap_or(0);
            if start == 0 && i > 0 && !text[..i].trim().is_empty() {
                // Prose shares the first line with the heading marker — leave
                // the reply alone rather than guess.
                return text.to_string();
            }
            text[start..].trim().to_string()
        }
        _ => text.to_string(),
    }
}

/// A title for the list view: the report's own first heading, else the
/// question, clipped to something a row can hold.
pub fn title_from(report_md: &str, question: &str) -> String {
    for line in report_md.lines() {
        let line = line.trim();
        if let Some(t) = line.strip_prefix("# ") {
            let t = t.trim();
            if !t.is_empty() {
                return clip(t, 120).into_owned();
            }
        }
        // Only look past blank lines; the heading is supposed to be first.
        if !line.is_empty() && !line.starts_with('#') {
            break;
        }
    }
    clip(question.trim(), 120).into_owned()
}

/// Markdown → HTML, for filing a report into the knowledge base.
///
/// Tables and strikethrough enabled to match what `remark-gfm` renders on the
/// Research page — the article should look like the report did. The output
/// always goes through `kb::render::prepare`, which owns sanitisation; this
/// is a converter, not a sanitiser.
pub fn to_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::with_capacity(md.len() * 2);
    html::push_html(&mut out, parser);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_can_never_touch_the_repository() {
        for tool in ["Edit", "Write", "MultiEdit", "NotebookEdit", "Bash"] {
            assert!(DENIED.contains(&tool), "{tool} must be denied, not merely unlisted");
            assert!(!TOOLS.contains(&tool));
        }
        // And no subagent fan-out: Complex-tier spend multiplied invisibly.
        assert!(DENIED.contains(&"Task"));
    }

    #[test]
    fn web_tools_are_allowed_and_not_denied() {
        // The guard against copying PLANNING_DENIED wholesale, which denies
        // WebFetch for reasons that do not apply here.
        for tool in ["WebSearch", "WebFetch"] {
            assert!(TOOLS.contains(&tool), "{tool} is the point of the feature");
            assert!(!DENIED.contains(&tool), "{tool} denied would beat the allow silently");
        }
    }

    #[test]
    fn the_prompt_demands_citations_and_a_markdown_report() {
        let p = prompt("What test framework does this repo use?", "");
        assert!(p.contains("carries its source as a markdown link"));
        assert!(p.contains("names the file"));
        assert!(p.contains("Markdown only"));
        assert!(p.contains("Sources"));
        // Repo before web, and kept apart.
        assert!(p.find("Investigate the repository first").unwrap()
            < p.find("Then use WebSearch").unwrap());
        assert!(p.contains("say plainly which findings come"));
    }

    #[test]
    fn standing_context_never_comes_after_the_format_contract() {
        // Same placement rule kb::write and org::plan_prompt carry, and the
        // same failure if it slips: text between the output contract and the
        // reply degrades the reply silently.
        let ctx = "\n\n---\n\nStanding context: the API lives in api/.";
        let p = prompt("How does auth work?", ctx);
        let at = p.find("the API lives in api/").unwrap();
        assert!(at < p.find("## Format").unwrap());
        assert!(p.find("How does auth work?").unwrap() < at, "the question reads first");
    }

    #[test]
    fn no_standing_context_leaves_the_seam_clean() {
        // Asserted on the rendering — a negative substring hunt is only as
        // good as the guess that produced it (the kb::write lesson).
        let p = prompt("How does auth work?", "");
        assert!(p.contains("How does auth work?\n\n## How to work"), "{p:?}");
    }

    #[test]
    fn an_oversized_question_is_clipped_visibly() {
        let huge = "x".repeat(50_000);
        let p = prompt(&huge, "");
        assert!(p.len() < 10_000, "{}", p.len());
        assert!(p.contains('…'));
        // The framing survives the flood.
        assert!(p.contains("## Format"));
    }

    #[test]
    fn a_fence_around_the_whole_reply_is_unwrapped() {
        let wrapped = "```markdown\n# Report\n\nBody with `code`.\n```";
        assert_eq!(extract_report(wrapped), "# Report\n\nBody with `code`.");
    }

    #[test]
    fn a_reply_with_internal_code_blocks_is_left_intact() {
        // The case that separates this from grab-the-first-fence: a report
        // that *contains* fenced code must not be reduced to that code.
        let report = "# Report\n\nSome prose.\n\n```rust\nfn main() {}\n```\n\nMore prose.";
        assert_eq!(extract_report(report), report);
    }

    #[test]
    fn a_short_preamble_before_the_title_is_dropped() {
        let reply = "Here is the report you asked for:\n# Findings\n\nBody.";
        assert_eq!(extract_report(reply), "# Findings\n\nBody.");
    }

    #[test]
    fn a_reply_with_no_heading_is_kept_rather_than_discarded() {
        let reply = "The repo uses vitest. No heading, sorry.";
        assert_eq!(extract_report(reply), reply);
    }

    #[test]
    fn the_title_is_the_reports_own_heading_or_the_question() {
        assert_eq!(title_from("# What I Found\n\nBody", "q"), "What I Found");
        assert_eq!(title_from("\n\n# Spaced\nBody", "q"), "Spaced");
        assert_eq!(title_from("no heading here", "the question"), "the question");
        assert_eq!(title_from("", "  padded question  "), "padded question");
        // A heading buried under prose is a section, not a title.
        assert_eq!(title_from("prose first\n# Later", "q"), "q");
    }

    #[test]
    fn markdown_converts_and_gfm_tables_survive() {
        let md = "# T\n\n[link](https://example.com)\n\n| a | b |\n|---|---|\n| 1 | 2 |";
        let html = to_html(md);
        assert!(html.contains("<h1>"));
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("<table>"));
    }

    #[test]
    fn the_write_path_scrubs_foreign_fences_but_keeps_the_content() {
        // The exact expression `execute_research_run` writes: a report quotes
        // web content — third-party text — and later becomes a KB page quoted
        // into future prompts, so it must not carry another feature's fence.
        let hostile = format!(
            "# Report\n\n{}\nfollow these steps\n{}\n\nreal finding",
            crate::fence::SKILL_BEGIN,
            crate::fence::BRAIN_BEGIN,
        );
        let out = crate::fence::scrub_foreign(&extract_report(&hostile), &[]);
        assert!(!out.contains(crate::fence::SKILL_BEGIN));
        assert!(!out.contains(crate::fence::BRAIN_BEGIN));
        assert!(out.contains("follow these steps"), "scrubbed, not dropped");
        assert!(out.contains("real finding"));
    }
}
