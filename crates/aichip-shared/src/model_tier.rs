use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Task-complexity tier that routes a run to a concrete model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    /// Mechanical, well-specified work.
    Easy,
    /// Typical feature/bugfix work.
    #[default]
    Medium,
    /// Architecture, judging, gnarly debugging.
    Complex,
}

/// Tier → model-ID mapping. Stored in settings so users on plans without
/// access to a given model can remap (e.g. Complex → Opus).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierMapping(pub BTreeMap<ModelTier, String>);

impl Default for TierMapping {
    fn default() -> Self {
        // Complex maps to Opus, not Fable. The most capable model is not the
        // right default for anyone: it is the one most likely to be outside a
        // given plan, and the one that turns an ordinary task into a bill you
        // didn't ask for. Fable is offered in settings for people who want it.
        Self(BTreeMap::from([
            (ModelTier::Easy, "claude-sonnet-5".to_string()),
            (ModelTier::Medium, "claude-opus-5".to_string()),
            (ModelTier::Complex, "claude-opus-5".to_string()),
        ]))
    }
}

/// A model the user may route a tier to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChoice {
    pub id: &'static str,
    pub label: &'static str,
    /// One line on when it earns its keep, shown beside the picker.
    pub blurb: &'static str,
}

/// What the settings picker offers.
///
/// A fixed list rather than a free-text field: a typo'd model id fails at
/// the point a run starts, minutes later and far from where it was entered.
pub const MODEL_CHOICES: &[ModelChoice] = &[
    ModelChoice {
        id: "claude-haiku-4-5-20251001",
        label: "Haiku 4.5",
        blurb: "Fastest and cheapest. Good for mechanical edits.",
    },
    ModelChoice {
        id: "claude-sonnet-5",
        label: "Sonnet 5",
        blurb: "Balanced. The usual choice for well-specified work.",
    },
    ModelChoice {
        id: "claude-opus-5",
        label: "Opus 5",
        blurb: "Strong general coding. The default for real feature work.",
    },
    ModelChoice {
        id: "claude-fable-5",
        label: "Fable 5",
        blurb: "Most capable, and the most expensive. Opt in deliberately.",
    },
];

/// Is this a model we offer? Guards the settings endpoint.
pub fn is_known_model(id: &str) -> bool {
    MODEL_CHOICES.iter().any(|m| m.id == id)
}

impl TierMapping {
    pub fn model_for(&self, tier: ModelTier) -> &str {
        self.0
            .get(&tier)
            .map(String::as_str)
            .unwrap_or("claude-opus-5")
    }
}

impl std::cmp::Ord for ModelTier {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

impl std::cmp::PartialOrd for ModelTier {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
