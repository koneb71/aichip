//! Gemini CLI's `stream-json`, normalised into [`AichipEvent`]s.
//!
//! The schema is gemini-cli's own (`packages/core/src/output/types.ts`), not
//! Claude Code's: every line is `{type, timestamp, …}` with `type` one of
//! `init`, `message`, `tool_use`, `tool_result`, `error`, `result`.
//!
//! Stateful where the others are not, for two reasons. Assistant text arrives
//! as `delta` chunks, and a chunk per event would show a sentence as twenty
//! messages, so chunks are gathered until something else happens. And only
//! `init` names the session, which the final `result` has to carry.

use crate::pump::{reason_from, LineParser};
use aichip_shared::{rate_limit_signal, AichipEvent, Usage};
use serde_json::Value;

#[derive(Default)]
pub struct GeminiStream {
    session_id: String,
    /// Delta chunks not yet sent as one message.
    pending: String,
    /// The last thing the model said, which is what the run reports.
    last_text: String,
    /// The last fatal `error` event: an invalid stream (an empty or blocked
    /// response) says why there and then ends with a bare error `result`.
    last_error: String,
}

impl GeminiStream {
    /// Send what has been gathered, if anything.
    fn flush(&mut self) -> Vec<AichipEvent> {
        let text = std::mem::take(&mut self.pending);
        if text.trim().is_empty() {
            return vec![];
        }
        self.last_text.clone_from(&text);
        vec![AichipEvent::AssistantText { text }]
    }
}

impl LineParser for GeminiStream {
    fn line(&mut self, line: &str) -> Vec<AichipEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return vec![];
        };
        let s = |k: &str| v.get(k).and_then(Value::as_str);
        match s("type") {
            Some("init") => {
                self.session_id = s("session_id").unwrap_or_default().to_string();
                vec![AichipEvent::RunStarted {
                    session_id: s("session_id").map(String::from),
                    model: s("model").map(String::from),
                }]
            }
            Some("message") if s("role") == Some("assistant") => {
                let content = s("content").unwrap_or_default();
                if v.get("delta").and_then(Value::as_bool) == Some(true) {
                    self.pending.push_str(content);
                    return vec![];
                }
                let mut events = self.flush();
                self.pending.push_str(content);
                events.extend(self.flush());
                events
            }
            Some("tool_use") => {
                let mut events = self.flush();
                events.push(AichipEvent::ToolCall {
                    tool_name: s("tool_name").unwrap_or("unknown").to_string(),
                    tool_use_id: s("tool_id").unwrap_or_default().to_string(),
                    input: v.get("parameters").cloned().unwrap_or(Value::Null),
                });
                events
            }
            Some("tool_result") => {
                let mut events = self.flush();
                let text = s("output")
                    .or_else(|| v.pointer("/error/message").and_then(Value::as_str))
                    .unwrap_or_default();
                events.push(AichipEvent::ToolResult {
                    tool_use_id: s("tool_id").unwrap_or_default().to_string(),
                    is_error: s("status") == Some("error"),
                    summary: clip(text),
                });
                events
            }
            Some("result") => {
                let mut events = self.flush();
                // A run that ends in an error still spent what its stats say
                // — on a turn limit, a whole session's worth. Sent as usage
                // before the ending, so the tally keeps it (provisional: no
                // completion reconciled it) rather than recording nothing.
                if v.get("status").and_then(Value::as_str) != Some("success") {
                    if let Some(u) = v.get("stats").map(usage).filter(|u| *u != Usage::default()) {
                        events.push(AichipEvent::UsageUpdated { usage: u });
                    }
                }
                events.push(self.result(&v));
                events
            }
            Some("error") => {
                if s("severity") == Some("error") {
                    self.last_error = s("message").unwrap_or_default().to_string();
                }
                self.flush()
            }
            // `error` events are non-fatal by definition — a fatal one ends in
            // a `result` — and `message` from the user is the prompt echoed.
            _ => self.flush(),
        }
    }

    fn finish(&mut self, exit_ok: bool, exit_code: Option<i32>, tail: &[String]) -> AichipEvent {
        // Every successful run ends with a `result`, so a clean exit without
        // one is still not a result anyone can report.
        let reason = match exit_code {
            // In stream-json mode a sign-in failure logs to stderr and exits
            // 41 with no event at all.
            Some(41) => "Gemini CLI is not signed in. Run `gemini` once in a terminal and \
choose how to log in, then start this again."
                .to_string(),
            Some(55) => "Gemini CLI refused to run in a folder it does not trust.".to_string(),
            Some(53) => "Gemini CLI reached its limit of turns for one session.".to_string(),
            _ if exit_ok => "Gemini CLI ended without reporting a result.".to_string(),
            code => reason_from(
                tail,
                &format!(
                    "Gemini CLI exited{} without reporting a result.",
                    code.map(|c| format!(" with code {c}")).unwrap_or_default()
                ),
            ),
        };
        AichipEvent::RunFailed { reason }
    }
}

impl GeminiStream {
    fn result(&mut self, v: &Value) -> AichipEvent {
        if v.get("status").and_then(Value::as_str) == Some("success") {
            return AichipEvent::RunCompleted {
                session_id: self.session_id.clone(),
                // Tokens only: Gemini never prices a run.
                cost_usd: None,
                usage: v.get("stats").map(usage).unwrap_or_default(),
                result_text: self.last_text.clone(),
            };
        }
        let kind = v
            .pointer("/error/type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let message = v
            .pointer("/error/message")
            .and_then(Value::as_str)
            .filter(|m| !m.trim().is_empty())
            .map(String::from)
            .unwrap_or_else(|| self.last_error.clone());
        // `TerminalQuotaError` and `RetryableQuotaError` name themselves.
        if kind.contains("Quota") || rate_limit_signal(&message) {
            return AichipEvent::RateLimited {
                reset_at: None,
                message: if message.is_empty() {
                    kind.to_string()
                } else {
                    message
                },
            };
        }
        AichipEvent::RunFailed {
            reason: match (message.is_empty(), kind.is_empty()) {
                (false, _) => message,
                (true, false) => format!("Gemini CLI reported an error ({kind})"),
                (true, true) => "Gemini CLI reported an error".to_string(),
            },
        }
    }
}

/// `result.stats` in aichip's terms.
///
/// Gemini's `input_tokens` is the whole prompt *including* the cached part,
/// and its `input` the rest; aichip, like Claude Code, counts the two apart.
/// Its `output_tokens` leaves out thinking, which is billed as output — so
/// output is taken as everything that was not prompt, when the total says.
/// Over-counting a cap by the tool-call prompt is the safe direction;
/// under-counting it by a pro model's thinking is not.
fn usage(stats: &Value) -> Usage {
    let g = |k: &str| stats.get(k).and_then(Value::as_u64);
    let prompt = g("input_tokens").unwrap_or(0);
    let cached = g("cached").unwrap_or(0);
    let output = g("output_tokens").unwrap_or(0);
    Usage {
        input_tokens: g("input").unwrap_or(prompt.saturating_sub(cached)),
        output_tokens: g("total_tokens")
            .map(|t| t.saturating_sub(prompt).max(output))
            .unwrap_or(output),
        cache_read_tokens: cached,
        cache_creation_tokens: 0,
    }
}

fn clip(text: &str) -> String {
    const MAX: usize = 400;
    let mut out: String = text.chars().take(MAX).collect();
    if text.chars().count() > MAX {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(fixture: &str) -> Vec<AichipEvent> {
        let mut p = GeminiStream::default();
        fixture.lines().flat_map(|l| p.line(l)).collect()
    }

    #[test]
    fn a_whole_run_reads_as_one() {
        let events = run(include_str!("fixtures/task.jsonl"));
        assert_eq!(
            events[0],
            AichipEvent::RunStarted {
                session_id: Some("0d7c5e2a-4b8f-4b51-9a77-2f1c0d9a6e31".into()),
                model: Some("gemini-2.5-pro".into()),
            }
        );
        // Three delta chunks are one message, sent when the tool call starts.
        assert_eq!(
            events[1],
            AichipEvent::AssistantText {
                text: "I'll read the README first.".into()
            }
        );
        assert!(
            matches!(&events[2], AichipEvent::ToolCall { tool_name, tool_use_id, input }
            if tool_name == "read_file" && tool_use_id == "read_file-1" && input["file_path"] == "README.md")
        );
        assert!(
            matches!(&events[3], AichipEvent::ToolResult { is_error: false, summary, .. }
            if summary.starts_with("# demo"))
        );
        assert!(
            matches!(&events[5], AichipEvent::ToolResult { is_error: true, summary, .. }
            if summary == "old_string not found")
        );
        match events.last().unwrap() {
            AichipEvent::RunCompleted {
                session_id,
                cost_usd,
                usage,
                result_text,
            } => {
                assert_eq!(session_id, "0d7c5e2a-4b8f-4b51-9a77-2f1c0d9a6e31");
                assert_eq!(*cost_usd, None);
                assert_eq!(result_text, "Done: the README now says hello.");
                // input 13745 of which 10656 cached; 412 output plus 1290 thinking.
                assert_eq!(usage.input_tokens, 3089);
                assert_eq!(usage.cache_read_tokens, 10656);
                assert_eq!(usage.output_tokens, 1702);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_quota_error_is_a_rate_limit_and_any_other_is_a_failure() {
        let events = run(include_str!("fixtures/quota.jsonl"));
        assert!(
            matches!(events.last(), Some(AichipEvent::RateLimited { message, .. })
            if message.contains("exhausted your daily quota"))
        );

        // The error class alone is enough, whatever the message says.
        let events = run(
            r#"{"type":"result","timestamp":"t","status":"error","error":{"type":"TerminalQuotaError","message":"Daily limit reached for gemini-2.5-pro."}}"#,
        );
        assert!(matches!(&events[..], [AichipEvent::RateLimited { .. }]));

        let events = run(
            r#"{"type":"result","timestamp":"t","status":"error","error":{"type":"FatalToolExecutionError","message":"no shell"}}"#,
        );
        assert_eq!(
            events,
            vec![AichipEvent::RunFailed {
                reason: "no shell".into()
            }]
        );
    }

    #[test]
    fn a_run_that_ends_in_error_keeps_its_tokens_and_its_reason() {
        let events = run(concat!(
            r#"{"type":"error","timestamp":"t","severity":"error","message":"Model stream ended with an empty response."}"#,
            "\n",
            r#"{"type":"result","timestamp":"t","status":"error","stats":{"total_tokens":900,"input_tokens":700,"output_tokens":150,"cached":100,"input":600}}"#,
        ));
        match &events[..] {
            [AichipEvent::UsageUpdated { usage }, AichipEvent::RunFailed { reason }] => {
                assert_eq!(usage.output_tokens, 200);
                assert_eq!(reason, "Model stream ended with an empty response.");
            }
            other => panic!("{other:?}"),
        }
        // No stats, no message, no kind: still a sentence, not "()".
        assert_eq!(
            run(r#"{"type":"result","timestamp":"t","status":"error"}"#),
            vec![AichipEvent::RunFailed {
                reason: "Gemini CLI reported an error".into()
            }]
        );
    }

    #[test]
    fn a_non_fatal_error_and_noise_change_nothing() {
        assert!(run(r#"{"type":"error","timestamp":"t","severity":"warning","message":"Maximum session turns"}"#).is_empty());
        assert!(run("Loaded cached credentials.").is_empty());
        assert!(
            run(r#"{"type":"message","timestamp":"t","role":"user","content":"hi"}"#).is_empty()
        );
    }

    #[test]
    fn without_a_result_the_exit_code_explains_it() {
        let mut p = GeminiStream::default();
        let reason = |e| match e {
            AichipEvent::RunFailed { reason } => reason,
            other => panic!("{other:?}"),
        };
        assert!(reason(p.finish(false, Some(41), &[])).contains("not signed in"));
        assert!(reason(p.finish(false, Some(55), &[])).contains("trust"));
        assert_eq!(
            reason(p.finish(false, Some(1), &["Error: boom".into()])),
            "Error: boom"
        );
        assert!(reason(p.finish(false, Some(1), &[])).contains("code 1"));
        assert!(reason(p.finish(true, Some(0), &[])).contains("without reporting"));
    }
}
