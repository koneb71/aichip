//! Cursor CLI's `stream-json`, normalised into [`AichipEvent`]s.
//!
//! Claude Code's envelope (`system`/`init`, `assistant`, `result`) with
//! Cursor's own tool calls: a `tool_call` line with `subtype` `started` or
//! `completed`, whose `tool_call` object is keyed by the tool —
//! `{"readToolCall":{"args":…,"result":…}}` — or, for others, a
//! `{"function":{"name","arguments"}}`.
//!
//! Token usage is the soft spot. Cursor's changelog (February 2026) says
//! per-turn input, output and cache totals are in the stream; the field names
//! are not documented. So [`usage`] reads the spellings a JSON API would use
//! and reports nothing rather than guessing when none is there.

use crate::pump::{reason_from, LineParser};
use aichip_shared::{rate_limit_signal, AichipEvent, Usage};
use serde_json::Value;

#[derive(Default)]
pub struct CursorStream {
    session_id: String,
    last_text: String,
}

impl LineParser for CursorStream {
    fn line(&mut self, line: &str) -> Vec<AichipEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return vec![];
        };
        if let Some(id) = v.get("session_id").and_then(Value::as_str) {
            self.session_id = id.to_string();
        }
        match v.get("type").and_then(Value::as_str) {
            Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => {
                vec![AichipEvent::RunStarted {
                    session_id: v
                        .get("session_id")
                        .and_then(Value::as_str)
                        .map(String::from),
                    model: v.get("model").and_then(Value::as_str).map(String::from),
                }]
            }
            Some("assistant") => self.assistant(&v),
            Some("tool_call") => tool_call(&v).into_iter().collect(),
            Some("result") => vec![self.result(&v)],
            _ => vec![],
        }
    }

    fn finish(&mut self, exit_ok: bool, exit_code: Option<i32>, tail: &[String]) -> AichipEvent {
        if tail.iter().any(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("not logged in") || l.contains("not authenticated")
        }) {
            return AichipEvent::RunFailed {
                reason: "Cursor CLI is not signed in. Run `cursor-agent login` in a terminal, \
then start this again."
                    .into(),
            };
        }
        let fallback = match exit_code {
            _ if exit_ok => "Cursor CLI ended without reporting a result.".to_string(),
            Some(c) => format!("Cursor CLI exited with code {c} without reporting a result."),
            None => "Cursor CLI was stopped before it reported a result.".to_string(),
        };
        AichipEvent::RunFailed {
            reason: if exit_ok {
                fallback
            } else {
                reason_from(tail, &fallback)
            },
        }
    }
}

impl CursorStream {
    fn assistant(&mut self, v: &Value) -> Vec<AichipEvent> {
        let mut events = vec![];
        for block in v
            .pointer("/message/content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if block.get("type").and_then(Value::as_str) == Some("text") {
                let text = block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !text.trim().is_empty() {
                    self.last_text = text.to_string();
                    events.push(AichipEvent::AssistantText {
                        text: text.to_string(),
                    });
                }
            }
        }
        if let Some(u) = v
            .get("usage")
            .or_else(|| v.pointer("/message/usage"))
            .and_then(usage)
        {
            events.push(AichipEvent::UsageUpdated { usage: u });
        }
        events
    }

    fn result(&mut self, v: &Value) -> AichipEvent {
        let text = v.get("result").and_then(Value::as_str).unwrap_or_default();
        let failed = v.get("is_error").and_then(Value::as_bool) == Some(true)
            || v.get("subtype").and_then(Value::as_str) != Some("success");
        if failed {
            let message = if text.is_empty() {
                "Cursor CLI reported an error".to_string()
            } else {
                text.to_string()
            };
            return if rate_limit_signal(&message) {
                AichipEvent::RateLimited {
                    reset_at: None,
                    message,
                }
            } else {
                AichipEvent::RunFailed { reason: message }
            };
        }
        AichipEvent::RunCompleted {
            session_id: self.session_id.clone(),
            cost_usd: None,
            usage: v.get("usage").and_then(usage).unwrap_or_default(),
            // Cursor's `result` is every message run together; the report is
            // the last thing it said, as for every other engine.
            result_text: if self.last_text.is_empty() {
                text.to_string()
            } else {
                self.last_text.clone()
            },
        }
    }
}

/// One `tool_call` line as a call or its result.
fn tool_call(v: &Value) -> Option<AichipEvent> {
    let id = v
        .get("call_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let (name, body) = v.get("tool_call")?.as_object()?.iter().next()?;
    match v.get("subtype").and_then(Value::as_str)? {
        "started" => {
            let (tool_name, input) = if name == "function" {
                let args = body.get("arguments");
                (
                    body.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("function")
                        .to_string(),
                    // `arguments` is a JSON string in the OpenAI shape.
                    args.and_then(Value::as_str)
                        .and_then(|s| serde_json::from_str(s).ok())
                        .or_else(|| args.cloned())
                        .unwrap_or(Value::Null),
                )
            } else {
                (
                    name.strip_suffix("ToolCall").unwrap_or(name).to_string(),
                    body.get("args").cloned().unwrap_or(Value::Null),
                )
            };
            Some(AichipEvent::ToolCall {
                tool_name,
                tool_use_id: id,
                input,
            })
        }
        "completed" => {
            let result = body.get("result");
            let error = result.and_then(|r| r.get("error").or_else(|| r.get("failure")));
            let summary = match (error, result.and_then(|r| r.get("success"))) {
                (Some(e), _) => describe(e),
                (None, Some(ok)) => describe(ok),
                (None, None) => String::new(),
            };
            Some(AichipEvent::ToolResult {
                tool_use_id: id,
                is_error: error.is_some(),
                summary,
            })
        }
        _ => None,
    }
}

/// A result object in a line: its text if it has some, else what it did.
fn describe(v: &Value) -> String {
    let text = match v {
        Value::String(s) => s.clone(),
        _ => ["content", "message", "output", "stdout"]
            .iter()
            .find_map(|k| v.get(*k).and_then(Value::as_str).map(String::from))
            .or_else(|| {
                let path = v.get("path").and_then(Value::as_str)?;
                Some(match v.get("linesCreated").and_then(Value::as_u64) {
                    Some(n) => format!("wrote {path} ({n} lines)"),
                    None => path.to_string(),
                })
            })
            .unwrap_or_default(),
    };
    const MAX: usize = 400;
    let mut out: String = text.chars().take(MAX).collect();
    if text.chars().count() > MAX {
        out.push('…');
    }
    out
}

/// Token counts under whichever spelling the line uses, or `None`.
fn usage(u: &Value) -> Option<Usage> {
    let g = |keys: &[&str]| keys.iter().find_map(|k| u.get(*k).and_then(Value::as_u64));
    let input = g(&["input_tokens", "inputTokens"]);
    let output = g(&["output_tokens", "outputTokens"]);
    if input.is_none() && output.is_none() {
        return None;
    }
    Some(Usage {
        input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        cache_read_tokens: g(&[
            "cache_read_input_tokens",
            "cache_read_tokens",
            "cacheReadTokens",
        ])
        .unwrap_or(0),
        cache_creation_tokens: g(&[
            "cache_creation_input_tokens",
            "cache_write_tokens",
            "cacheWriteTokens",
        ])
        .unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(fixture: &str) -> Vec<AichipEvent> {
        let mut p = CursorStream::default();
        fixture.lines().flat_map(|l| p.line(l)).collect()
    }

    #[test]
    fn a_whole_run_reads_as_one() {
        let events = run(include_str!("fixtures/task.jsonl"));
        assert_eq!(
            events[0],
            AichipEvent::RunStarted {
                session_id: Some("c6b62c6f-7ead-4fd6-9922-e952131177ff".into()),
                model: Some("Auto".into()),
            }
        );
        assert!(
            matches!(&events[1], AichipEvent::AssistantText { text } if text == "Reading the README.")
        );
        assert!(
            matches!(&events[2], AichipEvent::ToolCall { tool_name, tool_use_id, input }
            if tool_name == "read" && tool_use_id == "toolu_1" && input["path"] == "README.md")
        );
        assert!(
            matches!(&events[3], AichipEvent::ToolResult { is_error: false, summary, .. }
            if summary == "# demo\n")
        );
        assert!(
            matches!(&events[4], AichipEvent::ToolCall { tool_name, .. } if tool_name == "write")
        );
        assert!(matches!(&events[5], AichipEvent::ToolResult { summary, .. }
            if summary == "wrote README.md (3 lines)"));
        assert!(
            matches!(&events[6], AichipEvent::ToolCall { tool_name, input, .. }
            if tool_name == "run_terminal_cmd" && input["command"] == "git status")
        );
        assert!(
            matches!(&events[7], AichipEvent::ToolResult { is_error: true, summary, .. }
            if summary == "command not allowed")
        );
        match events.last().unwrap() {
            AichipEvent::RunCompleted {
                session_id,
                cost_usd,
                usage,
                result_text,
            } => {
                assert_eq!(session_id, "c6b62c6f-7ead-4fd6-9922-e952131177ff");
                assert_eq!(*cost_usd, None);
                assert_eq!(result_text, "The README now says hello.");
                assert_eq!((usage.input_tokens, usage.output_tokens), (5120, 311));
                assert_eq!(usage.cache_read_tokens, 4096);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_result_without_usage_reports_none_rather_than_a_guess() {
        let events = run(
            r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s"}"#,
        );
        assert!(
            matches!(&events[0], AichipEvent::RunCompleted { usage, .. } if *usage == Usage::default())
        );
    }

    #[test]
    fn an_error_result_fails_and_a_limit_is_a_rate_limit() {
        let failed = run(r#"{"type":"result","subtype":"error","is_error":true,"result":"boom"}"#);
        assert_eq!(
            failed,
            vec![AichipEvent::RunFailed {
                reason: "boom".into()
            }]
        );
        let limited = run(
            r#"{"type":"result","subtype":"error","is_error":true,"result":"Rate limit exceeded"}"#,
        );
        assert!(matches!(&limited[0], AichipEvent::RateLimited { .. }));
    }

    #[test]
    fn a_signed_out_run_says_so() {
        let mut p = CursorStream::default();
        match p.finish(false, Some(1), &["Error: Not logged in".into()]) {
            AichipEvent::RunFailed { reason } => assert!(reason.contains("cursor-agent login")),
            other => panic!("{other:?}"),
        }
    }
}
