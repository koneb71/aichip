use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Starting,
    Running,
    WaitingPermission,
    /// An organization run parked after planning, waiting for a human to
    /// approve the assignments. Unlike the other blocked states this one is
    /// durable: the run holds no concurrency permit and survives a restart,
    /// because a person may take hours to look.
    AwaitingApproval,
    RateLimited,
    Completed,
    Failed,
    Canceled,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }

    /// The run still owes an outcome — it blocks deleting or moving whatever
    /// it belongs to, even when nothing is executing right now.
    pub fn is_active(self) -> bool {
        !self.is_terminal()
    }

    /// An engine is burning tokens right now. Narrower than `is_active`: a
    /// parked or rate-limited run is active but idle, and showing it as
    /// "working" would be a lie.
    pub fn is_working(self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }

    /// Can the queue hand this run to `execute`?
    ///
    /// The guard `execute` was missing. A queue row can outlive the run it
    /// belongs to — `finish` never deleted one — and `execute` reads only the
    /// columns that decide *which kind* of run it is, so a terminal row popped
    /// off the queue five minutes later was re-dispatched and charged for
    /// again. Expressed here, next to the enum, so it is exhaustive and
    /// testable rather than a condition spelled out at the call site.
    pub fn is_dispatchable(self) -> bool {
        matches!(self, Self::Queued | Self::RateLimited)
    }

    /// The inverse of `as_str`, for a status read back out of the database.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => Self::Queued,
            "starting" => Self::Starting,
            "running" => Self::Running,
            "waiting_permission" => Self::WaitingPermission,
            "awaiting_approval" => Self::AwaitingApproval,
            "rate_limited" => Self::RateLimited,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "canceled" => Self::Canceled,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::WaitingPermission => "waiting_permission",
            Self::AwaitingApproval => "awaiting_approval",
            Self::RateLimited => "rate_limited",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

/// Every variant, so a test can loop them and a new one cannot be forgotten.
pub const ALL_RUN_STATUSES: [RunStatus; 9] = [
    RunStatus::Queued,
    RunStatus::Starting,
    RunStatus::Running,
    RunStatus::WaitingPermission,
    RunStatus::AwaitingApproval,
    RunStatus::RateLimited,
    RunStatus::Completed,
    RunStatus::Failed,
    RunStatus::Canceled,
];

#[cfg(test)]
mod tests {
    use super::RunStatus::*;
    use super::{RunStatus, ALL_RUN_STATUSES};

    #[test]
    fn a_parked_run_is_active_but_not_working() {
        assert!(AwaitingApproval.is_active());
        assert!(!AwaitingApproval.is_working());
        assert!(!AwaitingApproval.is_terminal());
    }

    #[test]
    fn only_starting_and_running_burn_tokens() {
        assert!(Starting.is_working() && Running.is_working());
        for status in [
            Queued,
            WaitingPermission,
            RateLimited,
            Completed,
            Failed,
            Canceled,
        ] {
            assert!(
                !status.is_working(),
                "{} should not be working",
                status.as_str()
            );
        }
    }

    #[test]
    fn terminal_statuses_are_inactive() {
        for status in [Completed, Failed, Canceled] {
            assert!(!status.is_active());
        }
    }

    #[test]
    fn a_status_survives_the_round_trip_through_the_database() {
        for st in ALL_RUN_STATUSES {
            assert_eq!(RunStatus::parse(st.as_str()), Some(st), "{}", st.as_str());
        }
        assert_eq!(RunStatus::parse("nonsense"), None);
    }

    #[test]
    fn only_a_queued_or_held_run_may_be_dispatched() {
        // The guard against a queue row outliving its run. Anything already
        // executing would be run twice; anything terminal would be paid for
        // twice, which is what actually happened.
        for st in ALL_RUN_STATUSES {
            assert_eq!(
                st.is_dispatchable(),
                matches!(st, Queued | RateLimited),
                "{} dispatchable?",
                st.as_str()
            );
        }
    }
}

/// The three user-facing permission presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Every sensitive action surfaces a prompt in the dashboard.
    #[default]
    Reviewed,
    /// File edits auto-approved; Bash and other tools still prompt.
    AutoEdit,
    /// --dangerously-skip-permissions. Only permitted inside aichip-managed
    /// worktrees, behind a per-project opt-in.
    FullAuto,
}
