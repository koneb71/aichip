//! The part of an adapter that is the same for every JSON-lines CLI: spawn
//! the official binary, read stdout line by line through the adapter's own
//! parser, watch stderr for a rate-limit signal and keep its tail, and decide
//! the terminal event from the exit status when the stream did not.
//!
//! The adapters for Gemini CLI, Cursor CLI, Qwen Code and Amp use it; the
//! older three keep their own pumps, which carry engine-specific history this
//! one has no reason to repeat.

use crate::{EngineProcess, ProcessHandle};
use async_trait::async_trait;
use eren_shared::ErenEvent;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

/// One CLI's stream, as a state machine over lines.
pub trait LineParser: Send + 'static {
    /// Zero or more events for one stdout line. Never fails: a line it
    /// cannot read is ignored, so a CLI that adds a field or an event type
    /// thins the stream rather than killing the run.
    fn line(&mut self, line: &str) -> Vec<ErenEvent>;

    /// The terminal event when stdout closed without one. Called once, after
    /// the process has exited, and only then — so a parser never has to
    /// remember whether it already ended the run. `stderr_tail` is the last
    /// lines the CLI wrote there, for a reason when the stream gave none.
    fn finish(
        &mut self,
        exit_ok: bool,
        exit_code: Option<i32>,
        stderr_tail: &[String],
    ) -> ErenEvent;
}

/// Does this event end a run, as the orchestrator reads it?
fn is_terminal(e: &ErenEvent) -> bool {
    matches!(
        e,
        ErenEvent::RunCompleted { .. }
            | ErenEvent::RunFailed { .. }
            | ErenEvent::RateLimited { .. }
    )
}

/// What stderr left behind once it closed.
#[derive(Default)]
struct Stderr {
    tail: Vec<String>,
    /// The last line that read as a rate limit.
    rate_limited: Option<String>,
}

/// Spawn `cmd` (built through `env_guard::command` by the caller) and pump it.
///
/// **stderr never ends a run on its own.** A CLI that retries a quota error,
/// or falls back to a smaller model, says so on stderr and carries on — Gemini
/// prints "possible quota limitations … switching to the flash model" and then
/// finishes the work. Treating that line as the outcome would hold a healthy
/// run in the queue and then, when the real result arrived, finish a run that
/// is also parked: the stranded-queue-row bug `hold_rate_limited` exists to
/// prevent. So a rate-limit line on stderr only decides the outcome of a run
/// that exited non-zero *without* the stream saying how it ended.
pub fn spawn(
    mut cmd: Command,
    mut parser: Box<dyn LineParser>,
    label: &'static str,
) -> anyhow::Result<EngineProcess> {
    cmd.stdin(Stdio::null()) // a run that waits for input never returns
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, rx) = mpsc::channel::<ErenEvent>(256);

    let (err_tx, err_rx) = tokio::sync::oneshot::channel::<Stderr>();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut seen = Stderr::default();
        while let Ok(Some(line)) = lines.next_line().await {
            if eren_shared::rate_limit_signal(&line) {
                seen.rate_limited = Some(line.clone());
            }
            seen.tail.push(line);
            if seen.tail.len() > 20 {
                seen.tail.remove(0);
            }
        }
        let _ = err_tx.send(seen);
    });

    let pid = child.id();
    tokio::spawn(async move {
        // An ending is held, not sent, until the process has exited, and only
        // the last one counts. Qwen writes a failed result line for a failed
        // sub-agent and carries on — sent at once, it ended a live run (and a
        // chat turn) and later ones followed it. The final word comes last.
        let mut ending: Option<ErenEvent> = None;
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            for event in parser.line(&line) {
                if is_terminal(&event) {
                    ending = Some(event);
                } else if tx.send(event).await.is_err() {
                    return; // receiver dropped — run canceled
                }
            }
        }
        let status = child.wait().await.ok();
        let seen = err_rx.await.unwrap_or_default();
        tracing::debug!(engine = label, stderr_tail = ?seen.tail, "engine exited");
        let exit_ok = status.is_some_and(|s| s.success());
        let last = match (ending, seen.rate_limited) {
            (Some(said), _) => said,
            (None, Some(message)) if !exit_ok => ErenEvent::RateLimited {
                reset_at: None,
                message,
            },
            _ => parser.finish(exit_ok, status.and_then(|s| s.code()), &seen.tail),
        };
        let _ = tx.send(last).await;
    });

    Ok(EngineProcess::new(rx, Box::new(PidHandle { pid })))
}

/// The last stderr lines worth showing, joined — or a fallback.
pub fn reason_from(tail: &[String], fallback: &str) -> String {
    let useful: Vec<&str> = tail
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    match useful.len() {
        0 => fallback.to_string(),
        n => useful[n.saturating_sub(3)..]
            .join(" · ")
            .chars()
            .take(500)
            .collect(),
    }
}

struct PidHandle {
    pid: Option<u32>,
}

#[async_trait]
impl ProcessHandle for PidHandle {
    async fn interrupt(&mut self) -> anyhow::Result<()> {
        // SIGINT so the CLI can save its session before exiting.
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

    #[test]
    fn a_reason_is_the_end_of_stderr_or_the_fallback() {
        assert_eq!(reason_from(&[], "exited"), "exited");
        let tail = vec![
            "".into(),
            "warming up".into(),
            "a".into(),
            "b".into(),
            "fatal: no".into(),
        ];
        assert_eq!(reason_from(&tail, "x"), "a · b · fatal: no");
    }

    /// A failed sub-agent's result line, then the session's own: one ending,
    /// the last. (Qwen does this; the parser is the Claude-compatible one.)
    #[cfg(unix)]
    #[tokio::test]
    async fn a_run_ends_once_with_the_last_word() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = concat!(
            r#"{"type":"system","subtype":"init","session_id":"s"}"#,
            "\n",
            r#"{"type":"result","subtype":"error_during_execution","is_error":true,"error":{"message":"TIMEOUT"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"carried on"}]}}"#,
            "\n",
            r#"{"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"s"}"#,
            "\n",
        );
        let bin = crate::replaying(dir.path(), fixture);
        let cmd = eren_shared::env_guard::command(&bin);
        let mut proc = spawn(
            cmd,
            Box::new(crate::claude::compat::ClaudeCompat { label: "Qwen Code" }),
            "test",
        )
        .unwrap();
        let mut events = vec![];
        while let Some(e) = proc.events.recv().await {
            events.push(e);
        }
        let endings: Vec<_> = events.iter().filter(|e| is_terminal(e)).collect();
        assert!(
            matches!(endings[..], [ErenEvent::RunCompleted { .. }]),
            "{events:?}"
        );
        assert!(matches!(
            events.last(),
            Some(ErenEvent::RunCompleted { .. })
        ));
    }
}
