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
pub mod codex;
pub mod mock;
pub mod opencode;

use aichip_shared::{AichipEvent, McpWiring, ModelTier, PermissionMode, ReasoningEffort};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct EngineInfo {
    pub version: String,
    pub authenticated: bool,
    /// Provider names and auth *type* only — never a credential. Empty for
    /// engines that don't expose this. Populated by running the CLI, never by
    /// reading its config or credential files.
    pub providers: Vec<ProviderInfo>,
    /// Model ids this install can actually reach right now, when the CLI can
    /// say. Empty means "we don't know" — never "none are available".
    ///
    /// This is what stops the tier defaults from being a guess: an engine
    /// fronting many providers has no fixed catalog, so the only honest
    /// source for "what can this machine run" is the machine.
    pub models: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderInfo {
    pub name: String,
    /// e.g. "api" or "oauth" — how the user authenticated, not the secret.
    pub auth: String,
}

/// What an engine can actually do.
///
/// Declared per adapter rather than discovered by `if engine == "..."` checks
/// scattered through the orchestrator. There is deliberately **no** `Default`
/// impl: a new adapter must state its own answers, because inheriting "yes I
/// can do everything" by omission is exactly how a descriptor like this rots
/// into a lie.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Capabilities {
    /// Can pause mid-run and ask a human to approve one tool call.
    /// `false` ⇒ `PermissionMode::Reviewed` cannot be honoured at all.
    pub interactive_permissions: bool,
    /// Emits a rate-limit signal carrying a reset time, so the queue can wait
    /// exactly as long as it needs to rather than guessing.
    pub structured_rate_limit: bool,
    /// Can resume a prior session by id.
    pub resume_sessions: bool,
    /// Can add to the system prompt without replacing the CLI's own.
    pub append_system_prompt: bool,
    /// Model ids come from a fixed catalog. `false` ⇒ free-text
    /// `provider/model`, which no catalog could keep up with.
    pub fixed_model_catalog: bool,
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
    /// How hard to think. `None` leaves the CLI's own default alone.
    pub effort: Option<ReasoningEffort>,
    pub resume_session_id: Option<String>,
    pub permission_mode: PermissionMode,
    pub allowed_tools: Vec<String>,
    /// Tools this run must not be able to use, whatever else is granted.
    ///
    /// Not the inverse of `allowed_tools`, and not redundant with it: Claude
    /// Code's `--allowedTools` is an *auto-approval* list, so naming three
    /// read-only tools there does not stop it reaching for `Bash`. Only an
    /// explicit denial does. Adapters must apply this last, so it beats
    /// anything the allow-list or the permission mode would otherwise permit.
    pub denied_tools: Vec<String>,
    pub append_system_prompt: Option<String>,
    /// Stable id for per-run scratch files (the MCP config). The run id for
    /// task/chat runs, the step id for an org member — whatever the caller
    /// uses to keep concurrent runs from colliding.
    pub run_key: String,
    /// What this run should reach over MCP. Each adapter renders it into its
    /// own config dialect — the orchestrator no longer knows any of them.
    pub mcp: McpWiring,
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
    /// Stable identifier: "claude-code" | "opencode" | "mock".
    fn id(&self) -> &'static str;

    /// Human-facing name, for pickers and error messages.
    fn label(&self) -> &'static str;

    /// What this engine can do. No default — see [`Capabilities`].
    fn capabilities(&self) -> Capabilities;

    /// Probe the CLI: is it installed and logged in? Implemented by running
    /// the binary, never by inspecting its config files.
    async fn detect(&self) -> Option<EngineInfo>;

    fn start(&self, spec: RunSpec) -> anyhow::Result<EngineProcess>;
}

/// Refuse a run the engine cannot honour, with a reason a person can act on.
///
/// The one place capability mismatches are decided, so the answer is the same
/// whether you hit it from the board, a chat, a team run or a bake-off.
///
/// Note what this deliberately does **not** do: downgrade. The orchestrator
/// downgrades `FullAuto` to `Reviewed` when the safety gate isn't satisfied,
/// which is a de-escalation and therefore safe. Quietly turning `Reviewed`
/// into `AutoEdit` because the engine can't ask would be the opposite — a
/// privilege escalation performed on the user's behalf, which is exactly the
/// kind of thing the compliance rules exist to prevent. So it errors.
pub fn vet(
    engine: &dyn Engine,
    permission_mode: PermissionMode,
    resuming: bool,
) -> Result<(), String> {
    let caps = engine.capabilities();
    if permission_mode == PermissionMode::Reviewed && !caps.interactive_permissions {
        return Err(format!(
            "{} can't run in Reviewed mode: it has no way to pause and ask you to \
approve a tool call mid-run — headless, it silently rejects every prompt \
instead. Choose Auto-edit (aichip pre-grants exactly the tools this run \
needs) or Don't-ask, or run this on Claude Code.",
            engine.label()
        ));
    }
    if resuming && !caps.resume_sessions {
        return Err(format!(
            "{} can't resume a previous session, so this would silently start over \
without the earlier context.",
            engine.label()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Capabilities);

    #[async_trait]
    impl Engine for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn label(&self) -> &'static str {
            "Fake Engine"
        }
        fn capabilities(&self) -> Capabilities {
            self.0
        }
        async fn detect(&self) -> Option<EngineInfo> {
            None
        }
        fn start(&self, _spec: RunSpec) -> anyhow::Result<EngineProcess> {
            anyhow::bail!("not a real engine")
        }
    }

    fn caps(interactive: bool, resume: bool) -> Capabilities {
        Capabilities {
            interactive_permissions: interactive,
            structured_rate_limit: false,
            resume_sessions: resume,
            append_system_prompt: true,
            fixed_model_catalog: false,
        }
    }

    #[test]
    fn an_engine_that_cannot_ask_refuses_reviewed_rather_than_downgrading() {
        let engine = Fake(caps(false, true));
        let err = vet(&engine, PermissionMode::Reviewed, false).unwrap_err();
        assert!(
            err.contains("Fake Engine"),
            "the message must name the engine"
        );
        assert!(err.contains("Auto-edit"), "and offer a way forward");
    }

    #[test]
    fn the_same_engine_is_fine_for_modes_it_can_honour() {
        let engine = Fake(caps(false, true));
        vet(&engine, PermissionMode::AutoEdit, false).unwrap();
        vet(&engine, PermissionMode::FullAuto, false).unwrap();
    }

    #[test]
    fn resuming_is_refused_only_when_actually_resuming() {
        let engine = Fake(caps(true, false));
        vet(&engine, PermissionMode::AutoEdit, false).unwrap();
        assert!(vet(&engine, PermissionMode::AutoEdit, true).is_err());
    }

    #[test]
    fn a_fully_capable_engine_passes_everything() {
        let engine = Fake(caps(true, true));
        for mode in [
            PermissionMode::Reviewed,
            PermissionMode::AutoEdit,
            PermissionMode::FullAuto,
        ] {
            vet(&engine, mode, true).unwrap();
        }
    }
}
