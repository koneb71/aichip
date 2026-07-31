use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

/// Engine-agnostic event stream. Every adapter normalizes its CLI's native
/// output into this union; the UI and persister consume only this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AichipEvent {
    RunStarted {
        session_id: Option<String>,
        model: Option<String>,
    },
    AssistantText {
        text: String,
    },
    ToolCall {
        tool_name: String,
        tool_use_id: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        is_error: bool,
        summary: String,
    },
    PermissionRequested {
        request_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    PermissionResolved {
        request_id: String,
        allowed: bool,
    },
    UsageUpdated {
        usage: Usage,
    },
    RunCompleted {
        session_id: String,
        cost_usd: Option<f64>,
        usage: Usage,
        result_text: String,
    },
    RunFailed {
        reason: String,
    },
    RateLimited {
        reset_at: Option<DateTime<Utc>>,
        message: String,
    },
    /// Where the user's plan stands, as the CLI itself reports it.
    ///
    /// Emitted for *every* `rate_limit_event`, including the routine ones,
    /// because "you have used most of your week" is only useful before it
    /// becomes "you are blocked". Previously the allowed case was discarded and
    /// the first thing anyone learned about their usage was a failed run.
    ///
    /// Telemetry, never an outcome: it does not stop or fail anything.
    UsageStatus {
        /// `five_hour`, `seven_day` — the CLI's own vocabulary.
        limit_type: String,
        /// `allowed` | `warning` | `blocked`.
        status: String,
        resets_at: Option<DateTime<Utc>>,
        /// The plan has spilled into paid overage.
        using_overage: bool,
    },
}

/// An event bound to a specific run, as persisted and sent over the WS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub run_id: Uuid,
    pub step_id: Option<Uuid>,
    /// Per-run monotonic sequence number; lets clients resume replay.
    pub seq: i64,
    pub ts: DateTime<Utc>,
    #[serde(flatten)]
    pub event: AichipEvent,
}
