//! Who is on the team, described well enough to delegate to.
//!
//! The manager used to see `name (role) — description` and nothing else, so
//! "assign it to whoever is best suited" was guesswork. It now sees how each
//! specialist is actually configured.

use super::plan::tier_label;
use aichip_shared::{ModelTier, ReasoningEffort};
use uuid::Uuid;

/// How much of a specialist's system prompt the manager gets to read. Long
/// enough to convey how they work, short enough that a ten-person roster
/// doesn't crowd out the goal.
pub const PROMPT_EXCERPT_CHARS: usize = 200;

#[derive(Debug, Clone)]
pub struct Member {
    pub agent_id: Uuid,
    pub name: String,
    /// The role this team gave them, not the agent's own description.
    pub title: String,
    pub description: String,
    pub tier: ModelTier,
    pub effort: Option<ReasoningEffort>,
    /// Carried here so running an assignment needs no second agent lookup.
    pub system_prompt: String,
}

/// Flatten whitespace and clip on a character boundary — a multi-byte name
/// must not panic the planner.
pub fn prompt_excerpt(system_prompt: &str, max_chars: usize) -> String {
    let flat = system_prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let clipped: String = flat.chars().take(max_chars).collect();
    format!("{}…", clipped.trim_end())
}

pub fn render_roster(members: &[Member]) -> String {
    members
        .iter()
        .map(|m| {
            let mut entry = format!("- {} ({}) · tier: {}", m.name, m.title, tier_label(m.tier));
            if let Some(effort) = m.effort {
                entry.push_str(&format!(" · effort: {}", effort.as_str()));
            }
            if !m.description.trim().is_empty() {
                entry.push_str(&format!("\n  About: {}", m.description.trim()));
            }
            let excerpt = prompt_excerpt(&m.system_prompt, PROMPT_EXCERPT_CHARS);
            if !excerpt.is_empty() {
                entry.push_str(&format!("\n  Works like: \"{excerpt}\""));
            }
            entry
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, system_prompt: &str) -> Member {
        Member {
            agent_id: Uuid::new_v4(),
            name: name.into(),
            title: "Backend".into(),
            description: "Builds services".into(),
            tier: ModelTier::Complex,
            effort: Some(ReasoningEffort::High),
            system_prompt: system_prompt.into(),
        }
    }

    #[test]
    fn the_roster_shows_how_each_specialist_is_configured() {
        let rendered = render_roster(&[member("Priya", "Prefer SQLModel. Ship tests.")]);
        assert!(rendered.contains("Priya (Backend)"));
        assert!(rendered.contains("tier: complex"));
        assert!(rendered.contains("effort: high"));
        assert!(rendered.contains("Builds services"));
        assert!(rendered.contains("Works like: \"Prefer SQLModel. Ship tests.\""));
    }

    #[test]
    fn an_excerpt_is_flattened_and_clipped_on_a_char_boundary() {
        assert_eq!(prompt_excerpt("a\n\n  b   c", 100), "a b c");

        let wide = "日本語".repeat(200);
        let excerpt = prompt_excerpt(&wide, 10);
        assert_eq!(excerpt.chars().count(), 11, "10 chars plus the ellipsis");
        assert!(excerpt.ends_with('…'));
    }

    #[test]
    fn a_bare_agent_still_renders() {
        let mut m = member("Rex", "");
        m.description = String::new();
        m.effort = None;
        let rendered = render_roster(&[m]);
        assert_eq!(rendered, "- Rex (Backend) · tier: complex");
    }
}
