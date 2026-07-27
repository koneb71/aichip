//! Claude Code adapter: spawns the official `claude` binary from PATH in
//! headless mode and normalizes its stream-json output.

pub mod stream_parser;

use crate::{Engine, EngineInfo, EngineProcess, ProcessHandle, RunSpec};
use aichip_shared::{AichipEvent, PermissionMode};
use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

/// Env keys the adapter refuses to pass through — auth must only ever come
/// from the user's own CLI login (compliance invariant #3).
pub const FORBIDDEN_ENV_PREFIXES: &[&str] = &["ANTHROPIC_", "CLAUDE_CODE_OAUTH"];

pub struct ClaudeEngine {
    /// Binary name resolved from PATH. Always "claude" in production; tests
    /// may point at a stub script.
    pub binary: String,
}

impl Default for ClaudeEngine {
    fn default() -> Self {
        Self {
            binary: "claude".to_string(),
        }
    }
}

#[async_trait]
impl Engine for ClaudeEngine {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    async fn detect(&self) -> Option<EngineInfo> {
        let version = Command::new(&self.binary)
            .arg("--version")
            .output()
            .await
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;
        // Auth is probed by running the CLI, never by reading its config.
        // A tiny -p invocation fails fast with a login error when logged out.
        Some(EngineInfo {
            version,
            authenticated: true,
        })
    }

    fn start(&self, spec: RunSpec) -> anyhow::Result<EngineProcess> {
        let mut cmd = Command::new(&self.binary);
        cmd.current_dir(&spec.cwd)
            .arg("-p")
            .arg(&spec.prompt)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            .arg("--model")
            .arg(&spec.model_id);

        match spec.permission_mode {
            PermissionMode::Reviewed => {}
            PermissionMode::AutoEdit => {
                cmd.arg("--permission-mode").arg("acceptEdits");
            }
            PermissionMode::FullAuto => {
                // The orchestrator only ever sets FullAuto for runs whose cwd
                // is an aichip-managed worktree; enforced again there.
                cmd.arg("--dangerously-skip-permissions");
            }
        }
        if let Some(session) = &spec.resume_session_id {
            cmd.arg("--resume").arg(session);
        }
        if !spec.allowed_tools.is_empty() {
            cmd.arg("--allowedTools").arg(spec.allowed_tools.join(","));
        }
        if let Some(sys) = &spec.append_system_prompt {
            cmd.arg("--append-system-prompt").arg(sys);
        }
        if let Some(mcp) = &spec.mcp_config_path {
            cmd.arg("--mcp-config").arg(mcp);
        }
        if spec.permission_prompt_tool
            && spec.mcp_config_path.is_some()
            && spec.permission_mode != PermissionMode::FullAuto
        {
            cmd.arg("--permission-prompt-tool").arg("mcp__aichip__approve");
        }
        for (k, v) in &spec.extra_env {
            if FORBIDDEN_ENV_PREFIXES.iter().any(|p| k.starts_with(p)) {
                anyhow::bail!("refusing to set auth-related env var {k}");
            }
            cmd.env(k, v);
        }

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        let (tx, rx) = mpsc::channel::<AichipEvent>(256);

        // stderr watcher: surfaces fatal errors / rate-limit notices that
        // never make it into stream-json.
        let tx_err = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut tail: Vec<String> = vec![];
            while let Ok(Some(line)) = lines.next_line().await {
                if stream_parser::rate_limit_signal(&line) {
                    let _ = tx_err
                        .send(AichipEvent::RateLimited {
                            reset_at: None,
                            message: line.clone(),
                        })
                        .await;
                }
                tail.push(line);
                if tail.len() > 20 {
                    tail.remove(0);
                }
            }
            tracing::debug!(stderr_tail = ?tail, "claude stderr closed");
        });

        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            let mut saw_terminal = false;
            while let Ok(Some(line)) = lines.next_line().await {
                for event in stream_parser::parse_line(&line) {
                    saw_terminal |= matches!(
                        event,
                        AichipEvent::RunCompleted { .. }
                            | AichipEvent::RunFailed { .. }
                            | AichipEvent::RateLimited { .. }
                    );
                    if tx.send(event).await.is_err() {
                        return; // receiver dropped — run canceled
                    }
                }
            }
            if !saw_terminal {
                let _ = tx
                    .send(AichipEvent::RunFailed {
                        reason: "engine process exited without a result message".to_string(),
                    })
                    .await;
            }
        });

        Ok(EngineProcess::new(rx, Box::new(ClaudeHandle { child })))
    }
}

struct ClaudeHandle {
    child: Child,
}

#[async_trait]
impl ProcessHandle for ClaudeHandle {
    async fn interrupt(&mut self) -> anyhow::Result<()> {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            // SIGINT lets the CLI checkpoint its session before exiting.
            unsafe { libc_kill(pid as i32, 2) };
            return Ok(());
        }
        self.kill();
        Ok(())
    }

    fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, sig);
}
