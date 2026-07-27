use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Starting,
    Running,
    WaitingPermission,
    RateLimited,
    Completed,
    Failed,
    Canceled,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Canceled)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::WaitingPermission => "waiting_permission",
            Self::RateLimited => "rate_limited",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
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
