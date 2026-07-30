use crate::ReasoningEffort;
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

/// Tier routing per engine.
///
/// `claude-opus-5` is not something OpenCode can be asked for — it wants
/// `provider/model`. So "medium" cannot mean one model globally; it means one
/// model *per engine*, which is also what makes a cross-engine bake-off
/// meaningful rather than a category error.
#[derive(Debug, Clone, Serialize)]
pub struct EngineTierMapping(pub BTreeMap<String, TierMapping>);

impl EngineTierMapping {
    pub fn model_for(&self, engine: &str, tier: ModelTier) -> String {
        self.0
            .get(engine)
            .map(|m| m.model_for(tier).to_string())
            .unwrap_or_else(|| Self::defaults_for(engine).model_for(tier).to_string())
    }

    /// The mapping for one engine, falling back to that engine's defaults.
    pub fn for_engine(&self, engine: &str) -> TierMapping {
        self.0
            .get(engine)
            .cloned()
            .unwrap_or_else(|| Self::defaults_for(engine))
    }

    pub fn defaults_for(engine: &str) -> TierMapping {
        match engine {
            "opencode" => TierMapping(BTreeMap::from([
                (ModelTier::Easy, "anthropic/claude-haiku-4-5".to_string()),
                (ModelTier::Medium, "anthropic/claude-sonnet-4-5".to_string()),
                (ModelTier::Complex, "anthropic/claude-sonnet-4-5".to_string()),
            ])),
            // Claude Code and the mock engine both speak Claude model ids.
            _ => TierMapping::default(),
        }
    }
}

impl Default for EngineTierMapping {
    fn default() -> Self {
        Self(BTreeMap::from([
            ("claude-code".to_string(), Self::defaults_for("claude-code")),
            ("opencode".to_string(), Self::defaults_for("opencode")),
        ]))
    }
}

/// How hard each tier thinks, per engine.
///
/// Deliberately a separate map from `EngineTierMapping` rather than a second
/// field on it. A tier answers two questions — which model, and how long it
/// gets to use it — but they are set at different times and by different
/// people, and folding them together would have changed the stored shape of a
/// setting every install already has.
///
/// A tier with no entry inherits: the machine-wide default if one is set,
/// otherwise whatever the CLI does on its own. So an empty map is the shipped
/// state and means "nothing pinned anywhere", not "nothing configured yet".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EngineTierEffort(pub BTreeMap<String, BTreeMap<ModelTier, ReasoningEffort>>);

impl EngineTierEffort {
    pub fn effort_for(&self, engine: &str, tier: ModelTier) -> Option<ReasoningEffort> {
        self.0.get(engine).and_then(|t| t.get(&tier)).copied()
    }

    /// One engine's row, empty when it has none.
    pub fn for_engine(&self, engine: &str) -> BTreeMap<ModelTier, ReasoningEffort> {
        self.0.get(engine).cloned().unwrap_or_default()
    }
}

/// Accepts both shapes, because installs predating per-engine routing stored
/// a flat `{easy,medium,complex}` and must keep working without a migration.
impl<'de> Deserialize<'de> for EngineTierMapping {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(de)?;
        // Nested: values are objects keyed by tier.
        if let Ok(nested) = serde_json::from_value::<BTreeMap<String, TierMapping>>(raw.clone()) {
            if !nested.is_empty() {
                return Ok(Self(nested));
            }
        }
        // Flat: the legacy shape belonged to Claude Code.
        let flat: TierMapping =
            serde_json::from_value(raw).map_err(serde::de::Error::custom)?;
        let mut map = BTreeMap::new();
        map.insert("claude-code".to_string(), flat);
        map.insert("opencode".to_string(), Self::defaults_for("opencode"));
        Ok(Self(map))
    }
}

/// Is `id` a model this engine can actually be asked for?
///
/// Claude Code has a fixed catalog worth validating against — a typo there
/// fails minutes later at run start. OpenCode fronts 75+ providers plus local
/// models, so any fixed list would be wrong within a week and would block
/// `ollama/…` outright. Validating the *shape* still catches the realistic
/// mistake, which is a Claude id pasted into the OpenCode field.
pub fn is_known_model_for(engine: &str, id: &str) -> bool {
    match engine {
        "opencode" => is_provider_model_shape(id),
        _ => is_known_model(id),
    }
}

/// `provider/model`: exactly one slash, both halves non-empty, no whitespace.
pub fn is_provider_model_shape(id: &str) -> bool {
    let mut parts = id.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(p), Some(m), None) => {
            !p.is_empty() && !m.is_empty() && !id.chars().any(char::is_whitespace)
        }
        _ => false,
    }
}

/// Choose a tier mapping from the models an install can actually reach.
///
/// The built-in OpenCode defaults name `anthropic/…`, which is only right for
/// someone authenticated with Anthropic. A user whose one provider is Google
/// would get three tiers pointing at models they cannot run — and would find
/// out when their first task failed, not when they installed it.
///
/// So: rank by substring against known coding models, best available wins,
/// and fall back to whatever *is* there rather than to nothing. Returns
/// `None` only when the list is empty, which means "we couldn't ask".
pub fn pick_defaults(available: &[String]) -> Option<TierMapping> {
    if available.is_empty() {
        return None;
    }
    // Family keywords rather than exact ids, so this doesn't need editing
    // every time a provider ships a point release. Roughly descending
    // capability within each tier.
    const STRONG: &[&str] = &["claude-opus", "claude-sonnet", "gpt-5", "coder", "-pro"];
    const FAST: &[&str] = &["claude-haiku", "gpt-5-mini", "-flash", "-mini"];

    // Not coding models. Handing one to a coding agent fails in a way that
    // reads as an aichip bug rather than a configuration one. Previews are
    // excluded too — a default that names one rots when it's withdrawn.
    const NOT_FOR_CODING: &[&str] = &[
        "image", "tts", "embed", "veo", "lyria", "live", "robotics", "translate",
        "computer-use", "deep-research", "preview",
    ];

    let usable = |id: &&String| !NOT_FOR_CODING.iter().any(|b| id.contains(b));
    let best = |prefs: &[&str]| -> Option<String> {
        prefs.iter().find_map(|p| {
            available
                .iter()
                .filter(usable)
                .filter(|id| id.contains(p))
                // Shortest match wins, so `gemini-3.6-flash` beats
                // `gemini-3.1-flash-lite`; ties go to the id that sorts last,
                // which for version-numbered families is the newer one.
                .min_by_key(|id| (id.len(), std::cmp::Reverse(id.as_str())))
                .cloned()
        })
    };

    let anything = available
        .iter()
        .filter(usable)
        .min_by_key(|id| (id.len(), std::cmp::Reverse(id.as_str())))
        .or_else(|| available.first())?;
    let strong = best(STRONG).unwrap_or_else(|| anything.clone());
    let fast = best(FAST).unwrap_or_else(|| strong.clone());
    Some(TierMapping(BTreeMap::from([
        (ModelTier::Easy, fast),
        (ModelTier::Medium, strong.clone()),
        (ModelTier::Complex, strong),
    ])))
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// The case that motivated this: a real Google-only install, taken
    /// verbatim from `opencode models` on 2026-07-29.
    #[test]
    fn a_google_only_install_gets_models_it_can_actually_run() {
        let available = ids(&[
            "opencode/big-pickle",
            "google/gemini-2.5-flash",
            "google/gemini-2.5-pro",
            "google/gemini-3-pro-image",
            "google/gemini-3.1-pro-preview",
            "google/gemini-3.6-flash",
            "google/gemini-embedding-001",
            "google/veo-3.1-generate-preview",
        ]);
        let m = pick_defaults(&available).unwrap();
        // The only non-preview "pro" in the list.
        assert_eq!(m.model_for(ModelTier::Medium), "google/gemini-2.5-pro");
        assert_eq!(m.model_for(ModelTier::Easy), "google/gemini-3.6-flash");
    }

    #[test]
    fn image_tts_and_preview_variants_are_never_chosen() {
        let available = ids(&[
            "google/gemini-3-pro-image",
            "google/gemini-3.1-pro-preview",
            "google/lyria-3-pro-preview",
        ]);
        let m = pick_defaults(&available).unwrap();
        // Nothing usable, so it falls back rather than returning None: a
        // model the user can see and change beats no mapping at all.
        assert!(m.model_for(ModelTier::Medium).starts_with("google/"));
    }

    #[test]
    fn anthropic_still_wins_when_it_is_there() {
        let available = ids(&[
            "google/gemini-2.5-pro",
            "anthropic/claude-sonnet-4-5",
            "anthropic/claude-haiku-4-5",
        ]);
        let m = pick_defaults(&available).unwrap();
        assert_eq!(m.model_for(ModelTier::Complex), "anthropic/claude-sonnet-4-5");
        assert_eq!(m.model_for(ModelTier::Easy), "anthropic/claude-haiku-4-5");
    }

    #[test]
    fn easy_falls_back_to_the_strong_model_rather_than_to_nothing() {
        let available = ids(&["anthropic/claude-opus-4-5"]);
        let m = pick_defaults(&available).unwrap();
        assert_eq!(m.model_for(ModelTier::Easy), "anthropic/claude-opus-4-5");
    }

    #[test]
    fn no_catalog_means_no_opinion() {
        assert!(pick_defaults(&[]).is_none());
    }
}

#[cfg(test)]
mod per_engine_tests {
    use super::*;

    #[test]
    fn each_engine_gets_ids_it_can_actually_use() {
        let m = EngineTierMapping::default();
        assert_eq!(m.model_for("claude-code", ModelTier::Medium), "claude-opus-5");
        // Not a Claude id — OpenCode would reject that outright.
        assert!(m.model_for("opencode", ModelTier::Medium).contains('/'));
    }

    #[test]
    fn an_unknown_engine_falls_back_rather_than_returning_nothing() {
        let m = EngineTierMapping::default();
        assert!(!m.model_for("some-future-engine", ModelTier::Easy).is_empty());
    }

    #[test]
    fn the_legacy_flat_setting_still_loads_and_belongs_to_claude() {
        // Installs predating per-engine routing stored this shape. Reading it
        // as anything other than Claude's would silently repoint every run.
        let flat = serde_json::json!({
            "easy": "claude-sonnet-5", "medium": "claude-opus-5", "complex": "claude-opus-5"
        });
        let m: EngineTierMapping = serde_json::from_value(flat).unwrap();
        assert_eq!(m.model_for("claude-code", ModelTier::Easy), "claude-sonnet-5");
        // And OpenCode still gets something usable rather than a Claude id.
        assert!(m.model_for("opencode", ModelTier::Easy).contains('/'));
    }

    #[test]
    fn the_nested_setting_round_trips() {
        let m = EngineTierMapping::default();
        let json = serde_json::to_value(&m).unwrap();
        let back: EngineTierMapping = serde_json::from_value(json).unwrap();
        assert_eq!(
            back.model_for("opencode", ModelTier::Complex),
            m.model_for("opencode", ModelTier::Complex)
        );
    }

    #[test]
    fn opencode_validates_shape_not_membership() {
        // The realistic mistake: a Claude id pasted into the OpenCode field.
        assert!(!is_known_model_for("opencode", "claude-opus-5"));
        assert!(is_known_model_for("opencode", "anthropic/claude-sonnet-4-5"));
        // A local model no catalog would ever list.
        assert!(is_known_model_for("opencode", "ollama/qwen3-coder"));
        assert!(!is_known_model_for("opencode", "too/many/slashes"));
        assert!(!is_known_model_for("opencode", "has space/model"));
        assert!(!is_known_model_for("opencode", "/leading"));
    }

    #[test]
    fn claude_still_validates_against_its_catalog() {
        assert!(is_known_model_for("claude-code", "claude-opus-5"));
        assert!(!is_known_model_for("claude-code", "gpt-5"));
    }

    #[test]
    fn a_tier_with_no_effort_inherits_rather_than_defaulting() {
        // The shipped state: nothing pinned anywhere. `None` here has to mean
        // "ask the next place", not "low" — a silent floor would be the one
        // outcome nobody chose.
        let empty = EngineTierEffort::default();
        assert_eq!(empty.effort_for("claude-code", ModelTier::Complex), None);

        let mut e = EngineTierEffort::default();
        e.0.insert(
            "claude-code".to_string(),
            BTreeMap::from([(ModelTier::Complex, ReasoningEffort::Max)]),
        );
        assert_eq!(
            e.effort_for("claude-code", ModelTier::Complex),
            Some(ReasoningEffort::Max)
        );
        // Set on one tier, silent on its siblings and on other engines.
        assert_eq!(e.effort_for("claude-code", ModelTier::Easy), None);
        assert_eq!(e.effort_for("opencode", ModelTier::Complex), None);
    }

    #[test]
    fn tier_efforts_survive_a_round_trip_through_settings() {
        let mut e = EngineTierEffort::default();
        e.0.insert(
            "opencode".to_string(),
            BTreeMap::from([
                (ModelTier::Easy, ReasoningEffort::Low),
                (ModelTier::Complex, ReasoningEffort::XHigh),
            ]),
        );
        let back: EngineTierEffort =
            serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(
            back.effort_for("opencode", ModelTier::Complex),
            Some(ReasoningEffort::XHigh)
        );
        assert_eq!(back.effort_for("opencode", ModelTier::Medium), None);
    }
}
