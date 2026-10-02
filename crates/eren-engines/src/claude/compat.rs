//! Claude Code's `stream-json`, as written by somebody else's CLI.
//!
//! Qwen Code and Amp both document their stream as Claude-compatible, and
//! where it is, [`super::stream_parser::parse_line`] reads it. This wrapper
//! adds the three things those CLIs do that Claude Code does not:
//!
//! - an error `result` carries its reason in `error` (a string for Amp,
//!   `{type?, message}` for Qwen) rather than in `result`, so the Claude
//!   parser alone would report "engine reported error (error_during_execution)";
//! - Amp can end a run with a `system` line of subtype `error_*`;
//! - nothing in the stream is guaranteed, so the pump's exit-status verdict
//!   needs a sentence that names the CLI.

use super::stream_parser::parse_line;
use crate::pump::{reason_from, LineParser};
use eren_shared::{rate_limit_signal, ErenEvent};
use serde_json::Value;

pub struct ClaudeCompat {
    /// The CLI's name, for a reason a person reads.
    pub label: &'static str,
}

impl LineParser for ClaudeCompat {
    fn line(&mut self, line: &str) -> Vec<ErenEvent> {
        if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
            if let Some(message) = error_of(&v) {
                return vec![if rate_limit_signal(&message) {
                    ErenEvent::RateLimited {
                        reset_at: None,
                        message,
                    }
                } else {
                    ErenEvent::RunFailed { reason: message }
                }];
            }
        }
        parse_line(line)
    }

    fn finish(&mut self, exit_ok: bool, exit_code: Option<i32>, tail: &[String]) -> ErenEvent {
        let fallback = match exit_code {
            _ if exit_ok => format!("{} ended without reporting a result.", self.label),
            Some(c) => format!(
                "{} exited with code {c} without reporting a result.",
                self.label
            ),
            None => format!("{} was stopped before it reported a result.", self.label),
        };
        ErenEvent::RunFailed {
            reason: if exit_ok {
                fallback
            } else {
                reason_from(tail, &fallback)
            },
        }
    }
}

/// The reason a failed `result` or `system` line gives in its `error` field,
/// if this line is one.
fn error_of(v: &Value) -> Option<String> {
    let kind = v.get("type").and_then(Value::as_str)?;
    let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
    let failed = match kind {
        "result" => {
            v.get("is_error").and_then(Value::as_bool) == Some(true) || subtype.starts_with("error")
        }
        "system" => subtype.starts_with("error"),
        _ => false,
    };
    if !failed {
        return None;
    }
    let message = match v.get("error")? {
        Value::String(s) => s.clone(),
        e => e.get("message").and_then(Value::as_str)?.to_string(),
    };
    (!message.trim().is_empty()).then_some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(l: &str) -> Vec<ErenEvent> {
        ClaudeCompat { label: "Amp" }.line(l)
    }

    #[test]
    fn an_error_field_is_the_reason_in_either_shape() {
        assert_eq!(
            line(
                r#"{"type":"result","subtype":"error_during_execution","is_error":true,"error":"tool crashed","session_id":"T-1"}"#
            ),
            vec![ErenEvent::RunFailed {
                reason: "tool crashed".into()
            }]
        );
        assert_eq!(
            line(
                r#"{"type":"result","subtype":"error_during_execution","is_error":true,"error":{"type":"AuthError","message":"no credentials"}}"#
            ),
            vec![ErenEvent::RunFailed {
                reason: "no credentials".into()
            }]
        );
        assert_eq!(
            line(
                r#"{"type":"system","subtype":"error_max_turns","error":"too many turns","session_id":"T-1"}"#
            ),
            vec![ErenEvent::RunFailed {
                reason: "too many turns".into()
            }]
        );
        assert!(matches!(
            line(
                r#"{"type":"result","is_error":true,"subtype":"error_during_execution","error":"[API Error: 429 Too Many Requests]"}"#
            )[..],
            [ErenEvent::RateLimited { .. }]
        ));
    }

    #[test]
    fn everything_else_is_the_claude_parser() {
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"8","session_id":"T-1","usage":{"input_tokens":10,"output_tokens":3}}"#;
        assert_eq!(line(ok), parse_line(ok));
        // An error result with no `error` field still fails, in Claude's words.
        assert!(matches!(
            line(r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#)[..],
            [ErenEvent::RunFailed { .. }]
        ));
        // A system line that is not an error is not one.
        assert!(matches!(
            line(r#"{"type":"system","subtype":"init","session_id":"T-1"}"#)[..],
            [ErenEvent::RunStarted { .. }]
        ));
    }

    #[test]
    fn a_finished_run_that_talks_about_rate_limits_is_finished() {
        let done = r#"{"type":"result","subtype":"success","is_error":false,"result":"Added rate limiting; returns 429 after 5 attempts, within quota.","session_id":"T-1"}"#;
        assert!(matches!(line(done)[..], [ErenEvent::RunCompleted { .. }]));
        // A failed one that says so is still a limit.
        let limited = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"429 Too Many Requests"}"#;
        assert!(matches!(line(limited)[..], [ErenEvent::RateLimited { .. }]));
    }

    #[test]
    fn the_exit_verdict_names_the_cli() {
        let mut p = ClaudeCompat { label: "Qwen Code" };
        match p.finish(false, Some(1), &[]) {
            ErenEvent::RunFailed { reason } => {
                assert!(reason.starts_with("Qwen Code exited with code 1"))
            }
            other => panic!("{other:?}"),
        }
    }
}
