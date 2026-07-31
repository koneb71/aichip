//! Recognising "you are being throttled" in whatever words a provider used.
//!
//! Lifted out of the Claude stream parser so a second adapter can reuse the
//! heuristic rather than copy it. Claude Code emits structured
//! `rate_limit_event` telemetry carrying a reset time; most other providers
//! only surface a sentence in an error, so string matching is the floor every
//! engine can stand on.

/// What a `rate_limit_event` status actually means for the run in front of us.
///
/// The distinction earns its keep because Claude Code reports three states and
/// only one of them is a refusal:
///
/// * `allowed` — routine telemetry, nothing is wrong;
/// * `allowed_warning` — **still allowed**, but you are near the limit;
/// * `rejected` — this request will not be served.
///
/// Treating `allowed_warning` as a refusal is a real mistake with a real cost:
/// it abandons work the provider was perfectly willing to do, and it does so at
/// exactly the moment the user most needs their remaining budget spent well.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitStatus {
    /// Serving normally.
    Allowed,
    /// Serving, but close enough to the limit to be worth showing.
    Warning,
    /// Not serving. Back off until the reset.
    Blocked,
}

impl LimitStatus {
    /// Classify whatever the CLI put in `status`.
    ///
    /// Anything beginning `allowed` is allowed — that prefix is the contract
    /// the CLI keeps, and a new `allowed_something` should not start failing
    /// runs the day it ships. An unknown status is treated as blocked, because
    /// the cost of a needless backoff is a delay and the cost of the opposite
    /// is hammering a limit that is already refusing us.
    pub fn parse(status: &str) -> Self {
        let s = status.trim().to_ascii_lowercase();
        match s.as_str() {
            "allowed" => Self::Allowed,
            _ if s.starts_with("allowed") => Self::Warning,
            _ => Self::Blocked,
        }
    }

    /// Should this stop the run?
    pub fn blocks(self) -> bool {
        matches!(self, Self::Blocked)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Warning => "warning",
            Self::Blocked => "blocked",
        }
    }
}

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
    fn a_warning_is_not_a_refusal() {
        // The bug this exists to stop: `allowed_warning` means the request
        // *will* be served, and aborting on it threw away work Claude was
        // willing to do.
        assert_eq!(LimitStatus::parse("allowed"), LimitStatus::Allowed);
        assert_eq!(LimitStatus::parse("allowed_warning"), LimitStatus::Warning);
        assert!(!LimitStatus::parse("allowed_warning").blocks());
        assert!(!LimitStatus::parse("allowed").blocks());

        assert_eq!(LimitStatus::parse("rejected"), LimitStatus::Blocked);
        assert!(LimitStatus::parse("rejected").blocks());
    }

    #[test]
    fn an_unfamiliar_status_errs_towards_backing_off() {
        // A status we have never seen might mean anything; a needless backoff
        // costs a delay, the opposite costs a wall.
        assert!(LimitStatus::parse("throttled").blocks());
        assert!(LimitStatus::parse("").blocks());
        // ...but a *new* allowed_ variant must not start failing runs.
        assert!(!LimitStatus::parse("allowed_soft").blocks());
        assert!(!LimitStatus::parse("ALLOWED_WARNING").blocks());
    }

    #[test]
    fn ordinary_output_does_not_trip_it() {
        assert!(!rate_limit_signal("wrote 3 files"));
        assert!(!rate_limit_signal("tests passed"));
        assert!(!rate_limit_signal(""));
    }
}
