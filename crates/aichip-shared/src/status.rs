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

#[cfg(test)]
mod tests {
    use super::RunStatus::*;

    #[test]
    fn a_parked_run_is_active_but_not_working() {
        assert!(AwaitingApproval.is_active());
        assert!(!AwaitingApproval.is_working());
        assert!(!AwaitingApproval.is_terminal());
    }

    #[test]
    fn only_starting_and_running_burn_tokens() {
        assert!(Starting.is_working() && Running.is_working());
        for status in [Queued, WaitingPermission, RateLimited, Completed, Failed, Canceled] {
            assert!(!status.is_working(), "{} should not be working", status.as_str());
        }
    }

    #[test]
    fn terminal_statuses_are_inactive() {
        for status in [Completed, Failed, Canceled] {
            assert!(!status.is_active());
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
