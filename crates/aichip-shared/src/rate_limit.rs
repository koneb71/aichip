//! Recognising "you are being throttled" in whatever words a provider used.
//!
//! Lifted out of the Claude stream parser so a second adapter can reuse the
//! heuristic rather than copy it. Claude Code emits structured
//! `rate_limit_event` telemetry carrying a reset time; most other providers
//! only surface a sentence in an error, so string matching is the floor every
//! engine can stand on.

/// Does this text look like a provider telling us to slow down?
///
/// Deliberately generous: treating a stray mention as a rate limit costs one
/// unnecessary backoff, while missing a real one means hammering a limit that
/// is already refusing us.
pub fn rate_limit_signal(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "rate limit",
        "rate_limit",
        "ratelimit",
        "usage limit",
        "overloaded",
        "429",
        // Non-Anthropic phrasings, which the Claude-only version missed:
        "quota",             // Google, generic
        "resource_exhausted", // Gemini / gRPC
        "resource exhausted",
        "tokens per min",    // OpenAI TPM
        "requests per min",  // OpenAI RPM
        "too many requests",
        "capacity",
    ]
    .iter()
    .any(|needle| t.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_phrasings_still_match() {
        assert!(rate_limit_signal("Claude AI usage limit reached"));
        assert!(rate_limit_signal("API Error: 429 rate_limit_error"));
        assert!(rate_limit_signal("Overloaded"));
    }

    #[test]
    fn other_providers_now_match_too() {
        // These are the ones the Claude-only heuristic let through, which
        // would have left an OpenCode run retrying into a wall.
        assert!(rate_limit_signal("RESOURCE_EXHAUSTED: quota exceeded"));
        assert!(rate_limit_signal("Rate limit reached: tokens per min (TPM)"));
        assert!(rate_limit_signal("Too Many Requests"));
    }

    #[test]
    fn ordinary_output_does_not_trip_it() {
        assert!(!rate_limit_signal("wrote 3 files"));
        assert!(!rate_limit_signal("tests passed"));
        assert!(!rate_limit_signal(""));
    }
}
