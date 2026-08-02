//! What an app is allowed to ask aichip for.
//!
//! A closed set, spelled out in one place. Everything an app can reach beyond
//! its own tables is one of these, and an app holds none of them until a person
//! grants it — the manifest *requests*, `app_grants` *grants*, and the two are
//! deliberately different tables so that a rebuild which starts asking for more
//! shows up as a question rather than as a widening.
//!
//! Reading its own rows needs no scope at all. Those tables exist because this
//! app declared them, hold only what this app put there, and are dropped with
//! it; gating them would be asking permission to use the thing you installed.

use std::fmt;

/// One grantable capability.
///
/// Deliberately not `Deserialize`: a scope arrives as text from a manifest an
/// agent wrote, and `parse` returning `None` for an unknown one is how a typo
/// becomes an error naming the typo instead of a silently missing permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    ReadProjects,
    ReadBoard,
    ReadRuns,
    ReadSpend,
    ReadAgents,
    ReadKb,
    WriteBoard,
    /// Start an agent run. Its own scope, never folded into `WriteBoard`.
    ///
    /// Putting a card on a board is cheap and reversible. Starting a run spends
    /// real money and executes code on this machine, and the fact that the
    /// prompt template is legible in the manifest does not make those the same
    /// decision. A person should be able to say yes to one and no to the other.
    RunAgents,
}

/// Every scope, in the order they should be offered.
///
/// Reads first, then writes, then the one that spends money — so the list a
/// person scans gets more serious as it goes down rather than in whatever order
/// the enum happened to be written.
pub const ALL: [Scope; 8] = [
    Scope::ReadProjects,
    Scope::ReadBoard,
    Scope::ReadRuns,
    Scope::ReadSpend,
    Scope::ReadAgents,
    Scope::ReadKb,
    Scope::WriteBoard,
    Scope::RunAgents,
];

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadProjects => "read:projects",
            Self::ReadBoard => "read:board",
            Self::ReadRuns => "read:runs",
            Self::ReadSpend => "read:spend",
            Self::ReadAgents => "read:agents",
            Self::ReadKb => "read:kb",
            Self::WriteBoard => "write:board",
            Self::RunAgents => "run:agents",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        ALL.into_iter().find(|scope| scope.as_str() == s)
    }

    /// Whether granting this lets an app change something.
    ///
    /// Used to say so louder in the UI. A read scope leaks; a write scope acts.
    pub fn is_write(self) -> bool {
        matches!(self, Self::WriteBoard | Self::RunAgents)
    }

    /// One line, in the second person, describing what saying yes allows.
    ///
    /// Written as consequences rather than as endpoint names, because "read:runs"
    /// tells a person nothing about what they are agreeing to.
    pub fn blurb(self) -> &'static str {
        match self {
            Self::ReadProjects => "See the names of your projects.",
            Self::ReadBoard => "Read your cards — titles and columns, never the prompts you typed.",
            Self::ReadRuns => "See which runs happened, what they cost, and whether they worked.",
            Self::ReadSpend => "See what you have spent.",
            Self::ReadAgents => "See your agents' names, never their instructions.",
            Self::ReadKb => "Read your knowledge base pages.",
            Self::WriteBoard => "Add cards to your backlog and move them between columns.",
            Self::RunAgents => "Start agent runs. This spends money and runs code on this machine.",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Scopes a manifest asks for that have not been granted.
///
/// Ordered and deduplicated, so the question a person is asked is stable
/// between two builds that request the same things in a different order.
pub fn ungranted(requested: &[Scope], granted: &[Scope]) -> Vec<Scope> {
    let mut out: Vec<Scope> = requested
        .iter()
        .copied()
        .filter(|s| !granted.contains(s))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scope_round_trips_through_its_text() {
        // The text is what is stored in a manifest and in app_grants, so a
        // scope that cannot be read back is a grant that silently disappears.
        for scope in ALL {
            assert_eq!(Scope::parse(scope.as_str()), Some(scope));
        }
    }

    #[test]
    fn an_unknown_scope_is_not_quietly_accepted() {
        assert_eq!(Scope::parse("read:everything"), None);
        assert_eq!(Scope::parse("write:board "), None);
        assert_eq!(Scope::parse("READ:BOARD"), None);
        assert_eq!(Scope::parse(""), None);
    }

    #[test]
    fn starting_a_run_is_not_part_of_writing_to_the_board() {
        // The whole reason it is a separate scope: an app that files cards must
        // not thereby be able to spend money.
        assert!(Scope::WriteBoard.is_write());
        assert!(Scope::RunAgents.is_write());
        assert_ne!(Scope::WriteBoard, Scope::RunAgents);
        assert!(!Scope::ReadBoard.is_write());
    }

    #[test]
    fn ungranted_is_stable_and_deduplicated() {
        let requested = [Scope::WriteBoard, Scope::ReadBoard, Scope::WriteBoard];
        let granted = [Scope::ReadBoard];
        assert_eq!(ungranted(&requested, &granted), vec![Scope::WriteBoard]);
        // Nothing outstanding reads as nothing to ask, not as an empty prompt.
        assert!(ungranted(&[Scope::ReadBoard], &[Scope::ReadBoard]).is_empty());
        assert!(ungranted(&[], &[Scope::ReadBoard]).is_empty());
    }

    #[test]
    fn every_scope_can_explain_itself() {
        // A grant nobody can read is a grant everybody clicks through.
        for scope in ALL {
            assert!(!scope.blurb().is_empty());
            assert!(scope.blurb().ends_with('.'));
        }
    }
}
