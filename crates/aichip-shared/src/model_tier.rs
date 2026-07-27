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
        Self(BTreeMap::from([
            (ModelTier::Easy, "claude-sonnet-5".to_string()),
            (ModelTier::Medium, "claude-opus-5".to_string()),
            (ModelTier::Complex, "claude-fable-5".to_string()),
        ]))
    }
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
