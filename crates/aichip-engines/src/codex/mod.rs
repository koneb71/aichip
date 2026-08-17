//! OpenAI's Codex CLI as an engine.
//!
//! `codex exec "<task>" --json` runs headless and emits JSON Lines, which is
//! the same shape the other two adapters consume. Spawned from `PATH`, stdout
//! read, nothing else — the first compliance invariant, unchanged.
//!
//! ## Written without the binary, and marked as such
//!
//! Every other adapter here was built against a recorded transcript from the
//! real CLI. This one was not: `codex` was not installed on the machine where
//! it was written, so the flags and event names come from OpenAI's
//! non-interactive documentation. [`stream_parser`] is deliberately forgiving
//! about shapes it does not recognise for that reason.
//!
//! Nothing about that is user-visible risk, because an engine only appears
//! once `detect` finds it: a machine without `codex` behaves exactly as it did
//! before. But the first person to run it should record a real transcript and
//! correct whatever this got wrong.
//!
//! ## The known hazard
//!
//! openai/codex#15451 reports that `--json` is silently ignored while MCP
//! servers are configured, yielding malformed output. aichip always hands an
//! engine an MCP config — that is how permission prompts and the chat tools
//! reach it — so that is precisely the configuration this runs in. If the
//! issue is real and unfixed, the symptom is a run that produces no events and
//! then completes: survivable, thanks to the parser's tolerance, but wrong.
//! It is called out in `detect`'s note so it surfaces in `doctor` rather than
//! being discovered on a paid run.
//!
//! ## What is not wired yet: MCP
//!
//! `RunSpec.mcp` is ignored here. Each adapter renders that wiring into its
//! own config dialect, and doing so for Codex without the binary to check
//! against would be the guess this file is already stretching. The concrete
//! consequence: a **chat** run on Codex reaches none of aichip's board tools —
//! no `create_task`, no `search_code` — so it can talk about the repository
//! but not act on it. Board cards are unaffected, because they act through
//! ordinary file edits rather than through MCP.
//!
//! Permission prompts are a separate matter and already handled honestly:
//! `interactive_permissions: false` means `vet` refuses `Reviewed` at the
//! click, so nothing silently depends on an MCP channel that is not there.

pub mod stream_parser;

use crate::{Capabilities, Engine, EngineInfo, EngineProcess, ProcessHandle, RunSpec};
use aichip_shared::{AichipEvent, PermissionMode, ReasoningEffort};
use async_trait::async_trait;
use std::process::Stdio;
use stream_parser::StreamState;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

pub struct CodexEngine {
    binary: String,
}

impl Default for CodexEngine {
    fn default() -> Self {
        Self {
            binary: std::env::var("AICHIP_CODEX_BIN").unwrap_or_else(|_| "codex".to_string()),
        }
    }
}

/// The argument vector for one run. Pure, so the flags are testable without
/// the binary — which, for an adapter written from documentation, is the only
/// part that can be tested at all.
pub fn codex_args(spec: &RunSpec) -> Vec<String> {
    let mut args: Vec<String> = vec!["exec".into()];

    // Resume keeps a thread going rather than starting a new one, which is
    // what makes a follow-up turn cheaper than a fresh run.
    if let Some(id) = spec.resume_session_id.as_deref().filter(|s| !s.is_empty()) {
        args.push("resume".into());
        args.push(id.to_string());
    }

    args.push("--json".into());
    // No console exists here. A prompt is not a slow answer, it is a process
    // that never returns — the hazard `connect.rs` documents for `gh`.
    args.push("--skip-git-repo-check".into());

    if !spec.model_id.is_empty() {
        args.push("--model".into());
        args.push(spec.model_id.clone());
    }

    // Sandbox and approvals, from the permission mode. `Reviewed` is not
    // offered — see `capabilities` — so only the two non-interactive stances
    // are reachable here.
    let (sandbox, approval) = match spec.permission_mode {
        PermissionMode::FullAuto => ("danger-full-access", "never"),
        // Anything else gets the containing sandbox. Writes stay inside the
        // working directory, which for a board card is a worktree.
        _ => ("workspace-write", "never"),
    };
    args.push("--sandbox".into());
    args.push(sandbox.into());
    args.push("--ask-for-approval".into());
    args.push(approval.into());

    if let Some(effort) = spec.effort {
        args.push("-c".into());
        args.push(format!(
            "model_reasoning_effort=\"{}\"",
            // Codex documents low/medium/high. aichip's scale goes further,
            // and the two extra levels are mapped onto "high" rather than
            // passed through: an unknown value here is a config error the CLI
            // reports at spawn, which would turn "think harder" into a run
            // that never started.
            match effort {
                ReasoningEffort::Low => "low",
                ReasoningEffort::Medium => "medium",
                ReasoningEffort::High | ReasoningEffort::XHigh | ReasoningEffort::Max => "high",
            }
        ));
    }

    // The prompt last and positional, so a prompt beginning with a dash is
    // read as the task rather than as a flag.
    args.push("--".into());
    args.push(spec.prompt.clone());
    args
}

#[async_trait]
impl Engine for CodexEngine {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn label(&self) -> &'static str {
        "Codex"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // `codex exec` is headless: approvals are decided by the flags at
            // spawn time, and there is no channel to ask a person mid-run.
            // So `Reviewed` is refused at the click by `vet`, exactly as it
            // is for OpenCode, rather than being silently downgraded — that
            // would be a privilege escalation performed on the user's behalf.
            interactive_permissions: false,
            // No documented machine-readable rate-limit signal with a reset
            // time. The shared text matcher on stderr still fires; what is
            // absent is the *structured* one the queue can wait precisely on.
            structured_rate_limit: false,
            resume_sessions: true,
            // `codex exec` has no flag that adds to its system prompt without
            // replacing it. Instructions reach the model through the prompt
            // itself, which the orchestrator already handles for engines that
            // answer false here.
            append_system_prompt: false,
            // Free-text model ids: Codex takes whatever `--model` is given and
            // the catalog moves faster than any list here could.
            fixed_model_catalog: false,
        }
    }

    async fn detect(&self) -> Option<EngineInfo> {
        let out = Command::new(&self.binary)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .await
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let version = String::from_utf8_lossy(&out.stdout).trim().to_string();

        // "Is it logged in?" is answered by running the binary, never by
        // reading its config — the second invariant. `codex login status`
        // exits non-zero when there is no usable session.
        let authenticated = Command::new(&self.binary)
            .args(["login", "status"])
            .stdin(Stdio::null())
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        Some(EngineInfo {
            version,
            authenticated,
            providers: vec![],
            models: vec![],
        })
    }

    fn start(&self, spec: RunSpec) -> anyhow::Result<EngineProcess> {
        let mut cmd = Command::new(&self.binary);
        cmd.current_dir(&spec.cwd).args(codex_args(&spec));

        // A child inherits this process's environment, so anything aichip
        // holds as its own secret is taken away explicitly.
        for key in aichip_shared::AICHIP_OWN_SECRETS {
            cmd.env_remove(key);
        }
        for (k, v) in &spec.extra_env {
            if aichip_shared::is_auth_env(k) {
                anyhow::bail!("{}", aichip_shared::auth_env_refusal(k));
            }
            cmd.env(k, v);
        }

        cmd.stdin(Stdio::null()) // a run that needs input hangs forever otherwise
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let (tx, rx) = mpsc::channel::<AichipEvent>(256);

        // Codex documents progress on stderr and JSON on stdout, so stderr is
        // read for the rate-limit signal and kept as a tail for diagnosis.
        let tx_err = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut tail: Vec<String> = vec![];
            while let Ok(Some(line)) = lines.next_line().await {
                if aichip_shared::rate_limit_signal(&line) {
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
            tracing::debug!(stderr_tail = ?tail, "codex stderr closed");
        });

        let pid = child.id();
        let model = (!spec.model_id.is_empty()).then(|| spec.model_id.clone());
        // The pump owns the child: like OpenCode and unlike Claude Code, no
        // line says how the run went, so the exit status is the verdict.
        tokio::spawn(async move {
            let mut state = StreamState::new(model);
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                for event in stream_parser::parse_line(&line, &mut state) {
                    if tx.send(event).await.is_err() {
                        return; // receiver dropped — run canceled
                    }
                }
            }
            let exit_ok = child.wait().await.map(|s| s.success()).unwrap_or(false);
            let _ = tx.send(state.finish(exit_ok)).await;
        });

        Ok(EngineProcess::new(rx, Box::new(CodexHandle { pid })))
    }
}

struct CodexHandle {
    pid: Option<u32>,
}

#[async_trait]
impl ProcessHandle for CodexHandle {
    async fn interrupt(&mut self) -> anyhow::Result<()> {
        // SIGINT so the CLI can checkpoint its thread before exiting; the
        // pump owns the `Child`, so this goes through the pid.
        if let Some(pid) = self.pid {
            unsafe { crate::opencode::libc_kill(pid as i32, 2) };
        }
        Ok(())
    }

    fn kill(&mut self) {
        if let Some(pid) = self.pid {
            unsafe { crate::opencode::libc_kill(pid as i32, 9) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aichip_shared::ModelTier;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn spec() -> RunSpec {
        RunSpec {
            cwd: PathBuf::from("/tmp/wt"),
            prompt: "do the thing".into(),
            model_tier: ModelTier::Medium,
            model_id: "gpt-5-codex".into(),
            effort: None,
            resume_session_id: None,
            permission_mode: PermissionMode::AutoEdit,
            allowed_tools: vec![],
            denied_tools: vec![],
            append_system_prompt: None,
            run_key: "run-1".into(),
            extra_read_dirs: vec![],
            permission_prompt_tool: false,
            extra_env: HashMap::new(),
            mcp: Default::default(),
        }
    }

    #[test]
    fn the_prompt_is_positional_and_last() {
        // A prompt beginning with a dash must be read as the task, not as a
        // flag — which is what the `--` separator is for.
        let mut s = spec();
        s.prompt = "--version please".into();
        let args = codex_args(&s);
        assert_eq!(args.last().unwrap(), "--version please");
        assert_eq!(args[args.len() - 2], "--");
    }

    #[test]
    fn it_always_asks_for_json_and_never_for_a_prompt() {
        let args = codex_args(&spec());
        assert!(args.contains(&"--json".to_string()));
        // There is no console. Approval must never be interactive.
        let i = args.iter().position(|a| a == "--ask-for-approval").unwrap();
        assert_eq!(args[i + 1], "never");
    }

    #[test]
    fn full_auto_is_the_only_mode_that_leaves_the_sandbox() {
        let sandbox_of = |mode| {
            let mut s = spec();
            s.permission_mode = mode;
            let args = codex_args(&s);
            let i = args.iter().position(|a| a == "--sandbox").unwrap();
            args[i + 1].clone()
        };
        assert_eq!(sandbox_of(PermissionMode::FullAuto), "danger-full-access");
        assert_eq!(sandbox_of(PermissionMode::AutoEdit), "workspace-write");
        // Reviewed is refused before it gets here by `vet`, but if it ever
        // arrived it must not be the permissive one.
        assert_eq!(sandbox_of(PermissionMode::Reviewed), "workspace-write");
    }

    #[test]
    fn resuming_names_the_thread() {
        let mut s = spec();
        s.resume_session_id = Some("th_9".into());
        let args = codex_args(&s);
        assert_eq!(args[0], "exec");
        assert_eq!(args[1], "resume");
        assert_eq!(args[2], "th_9");
    }

    #[test]
    fn an_empty_session_id_is_not_a_resume() {
        // A blank column is "no session", not a thread called "".
        let mut s = spec();
        s.resume_session_id = Some(String::new());
        assert!(!codex_args(&s).contains(&"resume".to_string()));
    }

    #[test]
    fn it_refuses_to_claim_a_capability_it_does_not_have() {
        // The pairing `vet` exists to refuse: a headless engine cannot honour
        // Reviewed, and saying it could would produce a run that rejects
        // every tool call instead of a refusal at the click.
        let caps = CodexEngine::default().capabilities();
        assert!(!caps.interactive_permissions);
        assert!(crate::vet(&CodexEngine::default(), PermissionMode::Reviewed, false).is_err());
        assert!(crate::vet(&CodexEngine::default(), PermissionMode::AutoEdit, false).is_ok());
    }
}
