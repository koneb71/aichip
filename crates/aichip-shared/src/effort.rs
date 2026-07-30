use serde::{Deserialize, Serialize};

/// How hard the model should think before answering.
///
/// Separate from [`crate::ModelTier`] on purpose: tier picks *which* model
/// (and most of the cost), effort picks how much reasoning it spends. A
/// planning step benefits far more from effort than from a bigger model, so
/// the two are worth controlling independently.
///
/// Maps to the CLI's `--effort` flag. An unrecognized value there is a
/// warning that falls back to the default, so this degrades safely on an
/// older CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    /// Spelled out because snake_case would make this `x_high`, which the
    /// CLI doesn't accept and the database wouldn't round-trip.
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    /// The stronger of two settings. Used to lift a manager's planning step
    /// above its configured default without ever quietly lowering it.
    pub fn at_least(self, floor: Self) -> Self {
        self.max(floor)
    }
}


/// Which of the four places decided a run's thinking budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffortSource {
    Agent,
    Card,
    Tier,
    Default,
}

impl EffortSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Card => "card",
            Self::Tier => "tier",
            Self::Default => "default",
        }
    }
}

/// Settle how hard a run thinks, most specific first.
///
/// Pure, and the only copy. The board resolves this to *show* what a card will
/// do while the orchestrator resolves it to *make* the card do it — two call
/// sites that must agree, and which had already drifted once when they were
/// written out separately.
///
/// Every level is optional and `None` means "ask the next one". All four being
/// silent is the shipped state and a real answer: leave the CLI alone.
pub fn resolve_effort(
    agent: Option<ReasoningEffort>,
    card: Option<ReasoningEffort>,
    tier: Option<ReasoningEffort>,
    machine_default: Option<ReasoningEffort>,
) -> (Option<ReasoningEffort>, EffortSource) {
    if let Some(e) = agent {
        return (Some(e), EffortSource::Agent);
    }
    if let Some(e) = card {
        return (Some(e), EffortSource::Card);
    }
    if let Some(e) = tier {
        return (Some(e), EffortSource::Tier);
    }
    (machine_default, EffortSource::Default)
}

#[cfg(test)]
mod tests {
    use super::ReasoningEffort::*;

    #[test]
    fn round_trips_every_level_the_cli_accepts() {
        for level in [Low, Medium, High, XHigh, Max] {
            assert_eq!(super::ReasoningEffort::parse(level.as_str()), Some(level));
        }
    }

    #[test]
    fn parsing_is_forgiving_about_case_and_padding() {
        assert_eq!(super::ReasoningEffort::parse(" XHigh "), Some(XHigh));
        assert_eq!(super::ReasoningEffort::parse("nonsense"), None);
    }

    #[test]
    fn at_least_raises_but_never_lowers() {
        assert_eq!(Low.at_least(High), High);
        assert_eq!(Max.at_least(High), Max, "a deliberate Max must not be capped");
    }

    #[test]
    fn serde_uses_the_cli_spelling() {
        assert_eq!(serde_json::to_string(&XHigh).unwrap(), "\"xhigh\"");
    }

    #[test]
    fn the_most_specific_place_wins_and_names_itself() {
        use super::{resolve_effort, EffortSource};
        // All four silent: leave the CLI alone, and say the default decided it.
        assert_eq!(
            resolve_effort(None, None, None, None),
            (None, EffortSource::Default)
        );
        assert_eq!(
            resolve_effort(None, None, None, Some(High)),
            (Some(High), EffortSource::Default)
        );
        assert_eq!(
            resolve_effort(None, None, Some(Max), Some(High)),
            (Some(Max), EffortSource::Tier)
        );
        assert_eq!(
            resolve_effort(None, Some(Low), Some(Max), Some(High)),
            (Some(Low), EffortSource::Card)
        );
        assert_eq!(
            resolve_effort(Some(Medium), Some(Low), Some(Max), Some(High)),
            (Some(Medium), EffortSource::Agent)
        );
    }
}
