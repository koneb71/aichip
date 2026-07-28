//! Engine adapter layer.
//!
//! COMPLIANCE INVARIANTS (contribution rules — PRs violating these are rejected):
//! 1. Adapters spawn official agent binaries found on `PATH` and read their stdout.
//!    Nothing else.
//! 2. Never read, store, extract, or forward credentials. Never touch `~/.claude`
//!    or any engine's config/credential files.
//! 3. Never set authentication environment variables on spawned processes.
//! 4. Never proxy, intercept, or replay the engine's network traffic.

pub mod claude;
pub mod mock;

use aichip_shared::{AichipEvent, ModelTier, PermissionMode};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct EngineInfo {
    pub version: String,
    pub authenticated: bool,
}

#[derive(Debug, Clone)]
pub struct RunSpec {
    /// Working directory for the run — for board tasks this is an
    /// aichip-managed git worktree, never the user's main checkout.
    pub cwd: PathBuf,
    pub prompt: String,
    pub model_tier: ModelTier,
    /// Concrete model ID resolved from the user's tier mapping.
    pub model_id: String,
    pub resume_session_id: Option<String>,
    pub permission_mode: PermissionMode,
    pub allowed_tools: Vec<String>,
    pub append_system_prompt: Option<String>,
    /// Path to a generated MCP config pointing at one of aichip's MCP
    /// endpoints (permission proxy for task runs, workspace tools for chat).
    pub mcp_config_path: Option<PathBuf>,
    /// Directories outside `cwd` the run legitimately needs to read — today
    /// only the per-attachment dirs under `~/.aichip/attachments`.
    ///
    /// Declaring them is belt-and-braces: current Claude Code versions let an
    /// allowed `Read` reach any absolute path, so attachments resolve without
    /// this. We still say so explicitly, because that default is the CLI's to
    /// change and a version that tightened it would break attachments silently.
    pub extra_read_dirs: Vec<PathBuf>,
    /// Route permission prompts through aichip's MCP approve tool. Task runs
    /// set true; chat/utility runs set false (they rely on an explicit
    /// allowed-tools list and headless auto-deny instead).
    pub permission_prompt_tool: bool,
    /// Non-auth environment (e.g. AICHIP_RUN_ID for hooks). Adapters must
    /// refuse auth-related keys; see `claude::FORBIDDEN_ENV_PREFIXES`.
    pub extra_env: HashMap<String, String>,
}

/// A running engine process. Dropping it does not kill the child; call
/// `kill()` or let it run to completion.
pub struct EngineProcess {
    /// Normalized event stream. Closed channel = process finished.
    pub events: mpsc::Receiver<AichipEvent>,
    handle: Box<dyn ProcessHandle>,
}

impl EngineProcess {
    pub fn new(events: mpsc::Receiver<AichipEvent>, handle: Box<dyn ProcessHandle>) -> Self {
        Self { events, handle }
    }

    /// Graceful stop (SIGINT — lets the CLI checkpoint its session).
    pub async fn interrupt(&mut self) -> anyhow::Result<()> {
        self.handle.interrupt().await
    }

    /// Hard stop.
    pub fn kill(&mut self) {
        self.handle.kill();
    }
}

#[async_trait]
pub trait ProcessHandle: Send {
    async fn interrupt(&mut self) -> anyhow::Result<()>;
    fn kill(&mut self);
}

#[async_trait]
pub trait Engine: Send + Sync {
    /// Stable identifier: "claude-code" | "mock" | (later) "codex".
    fn id(&self) -> &'static str;

    /// Probe the CLI: is it installed and logged in? Implemented by running
    /// the binary, never by inspecting its config files.
    async fn detect(&self) -> Option<EngineInfo>;

    fn start(&self, spec: RunSpec) -> anyhow::Result<EngineProcess>;
}
