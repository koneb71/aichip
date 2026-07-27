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
