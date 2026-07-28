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
}
