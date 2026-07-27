use chrono::{DateTime, Duration, Utc};
use rand::Rng;

/// Backoff for rate-limited runs: 5m → 15m → 45m (capped), with jitter so a
/// burst of queued runs doesn't stampede the moment the window resets.
pub fn rate_limit_backoff(attempt: u32, reset_at: Option<DateTime<Utc>>) -> DateTime<Utc> {
    let jitter = Duration::seconds(rand::rng().random_range(5..90));
    match reset_at {
        Some(t) => t + jitter,
        None => {
            let minutes = match attempt {
                0 => 5,
                1 => 15,
                _ => 45,
            };
            Utc::now() + Duration::minutes(minutes) + jitter
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_escalates_and_caps() {
        let now = Utc::now();
        let b0 = rate_limit_backoff(0, None) - now;
        let b1 = rate_limit_backoff(1, None) - now;
        let b9 = rate_limit_backoff(9, None) - now;
        assert!(b0 >= Duration::minutes(5) && b0 < Duration::minutes(7));
        assert!(b1 >= Duration::minutes(15) && b1 < Duration::minutes(17));
        assert!(b9 >= Duration::minutes(45) && b9 < Duration::minutes(47));
    }

    #[test]
    fn backoff_honors_reset_time() {
        let reset = Utc::now() + Duration::hours(2);
        let t = rate_limit_backoff(0, Some(reset));
        assert!(t > reset && t < reset + Duration::minutes(2));
    }
}
