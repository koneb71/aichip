//! `codex exec --json` emits JSON Lines; this turns them into `AichipEvent`.
//!
//! ## Written against the documented shapes, not an observed run
//!
//! Every other adapter in this crate was built by running the real CLI and
//! recording what came back — that is the repository's rule, and this file is
//! the exception. Codex was not installed on the machine where it was written,
//! so the event names here come from OpenAI's non-interactive documentation
//! rather than from a transcript.
//!
//! Two consequences, and both are designed for rather than hoped away:
//!
//! - **Unknown lines are ignored, never fatal.** If the real stream carries a
//!   shape this does not know, the run keeps going and the text still lands —
//!   the failure mode is a thinner event stream, not a dead run.
//!   `parse_line` returns an empty vector for anything it cannot read,
//!   including lines that are not JSON at all.
//! - **Both event spellings are accepted.** The docs describe events as both
//!   `thread.started`/`item.completed` and a JSON-RPC-ish
//!   `{"method":"turn/started"}`. Rather than bet on one, the type is read
//!   from `type` or `method` and `.`/`/` are treated as the same separator.
//!
//! When somebody with `codex` installed runs it, the honest first step is to
//! capture a real transcript into a fixture here and delete whichever half of
//! this turned out to be wrong.

use aichip_shared::{AichipEvent, Usage};
use serde_json::Value;

/// What has to survive between lines.
pub struct StreamState {
    /// From `thread.started`, and what a later run resumes with.
    pub session_id: Option<String>,
    pub model: Option<String>,
    /// The assistant's text, accumulated so the terminal event can carry the
    /// whole answer the way `RunCompleted.result_text` expects.
    text: String,
    usage: Usage,
    /// Set by an explicit failure event, so `finish` can report the reason
    /// rather than only the exit code.
    failure: Option<String>,
}

impl StreamState {
    pub fn new(model: Option<String>) -> Self {
        Self {
            session_id: None,
            model,
            text: String::new(),
            usage: Usage::default(),
            failure: None,
        }
    }

    /// The terminal event, decided by the process's exit status.
    ///
    /// Like OpenCode and unlike Claude Code: there is no single line that
    /// says how the whole run went, so the exit code is the authority. A
    /// failure event seen mid-stream supplies the *reason* when there is one,
    /// because "exited non-zero" is not something a person can act on.
    pub fn finish(self, exit_ok: bool) -> AichipEvent {
        if exit_ok && self.failure.is_none() {
            AichipEvent::RunCompleted {
                session_id: self.session_id.unwrap_or_default(),
                // Codex reports tokens, not money. Left None rather than
                // guessed from a price table that would go stale — the same
                // choice the OpenCode adapter makes, and the reason the
                // activity view counts unpriced runs separately instead of
                // folding them in as zero.
                cost_usd: None,
                usage: self.usage,
                result_text: self.text,
            }
        } else {
            AichipEvent::RunFailed {
                reason: self
                    .failure
                    .unwrap_or_else(|| "codex exited without completing the turn".to_string()),
            }
        }
    }
}

/// Read the event type from either spelling, normalised to dots.
fn kind(v: &Value) -> Option<String> {
    let raw = v
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| v.get("method").and_then(Value::as_str))?;
    Some(raw.replace('/', "."))
}

/// The payload, which is at the top level in one spelling and under `params`
/// in the other.
fn payload<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)
        .or_else(|| v.get("params").and_then(|p| p.get(key)))
}

/// Text out of an item, whichever field it used.
fn item_text(item: &Value) -> Option<String> {
    for key in ["text", "content", "message", "delta"] {
        if let Some(s) = item.get(key).and_then(Value::as_str) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn usage_from(v: &Value) -> Option<Usage> {
    let u = payload(v, "usage")?;
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    Some(Usage {
        input_tokens: n("input_tokens") + n("inputTokens"),
        output_tokens: n("output_tokens") + n("outputTokens"),
        cache_read_tokens: n("cached_input_tokens") + n("cachedInputTokens"),
        cache_creation_tokens: 0,
    })
}

/// One line in, zero or more events out.
///
/// Zero is the normal answer for a line this does not recognise. That is the
/// whole robustness story for an adapter written without the binary: a stream
/// full of unknown shapes degrades to "the run produced no detail", which is
/// survivable, rather than to an error, which is not.
pub fn parse_line(line: &str, state: &mut StreamState) -> Vec<AichipEvent> {
    let line = line.trim();
    if line.is_empty() {
        return vec![];
    }
    let Ok(v): Result<Value, _> = serde_json::from_str(line) else {
        // Not JSON. `codex exec` documents progress on stderr and JSON on
        // stdout, but a banner or a warning on the wrong stream must not kill
        // a run.
        return vec![];
    };
    let Some(kind) = kind(&v) else { return vec![] };
    let mut out = Vec::new();

    if let Some(usage) = usage_from(&v) {
        if usage.input_tokens + usage.output_tokens > 0 {
            state.usage = usage.clone();
            out.push(AichipEvent::UsageUpdated { usage });
        }
    }

    match kind.as_str() {
        "thread.started" => {
            let id = payload(&v, "thread_id")
                .or_else(|| payload(&v, "threadId"))
                .or_else(|| payload(&v, "thread").and_then(|t| t.get("id")))
                .and_then(Value::as_str)
                .map(str::to_string);
            state.session_id = id.clone();
            out.push(AichipEvent::RunStarted {
                session_id: id,
                model: state.model.clone(),
            });
        }
        // A turn starting is not the run starting — a resumed thread has many.
        // Nothing to emit; the run already announced itself.
        "turn.started" => {}
        "item.started" | "item.completed" | "item.updated" => {
            let Some(item) = payload(&v, "item") else {
                return out;
            };
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match item_type {
                // Only on completion: an `item.started` for a message carries
                // no text yet, and emitting on both would duplicate it.
                "agentMessage" | "agent_message" | "assistantMessage" => {
                    if kind.ends_with(".completed") {
                        if let Some(text) = item_text(item) {
                            state.text.push_str(&text);
                            out.push(AichipEvent::AssistantText { text });
                        }
                    }
                }
                "commandExecution" | "command_execution" | "fileChange" | "file_change"
                | "toolCall" | "tool_call" | "mcpToolCall" => {
                    let tool_name = item
                        .get("name")
                        .or_else(|| item.get("tool"))
                        .and_then(Value::as_str)
                        .unwrap_or(item_type)
                        .to_string();
                    if kind.ends_with(".started") {
                        out.push(AichipEvent::ToolCall {
                            tool_name,
                            tool_use_id: id,
                            input: item
                                .get("input")
                                .or_else(|| item.get("command"))
                                .cloned()
                                .unwrap_or(Value::Null),
                        });
                    } else if kind.ends_with(".completed") {
                        let failed = item
                            .get("status")
                            .and_then(Value::as_str)
                            .is_some_and(|s| s.eq_ignore_ascii_case("failed"));
                        out.push(AichipEvent::ToolResult {
                            tool_use_id: id,
                            is_error: failed,
                            summary: item_text(item).unwrap_or_else(|| tool_name.clone()),
                        });
                    }
                }
                // A reasoning item is not shown as the answer: it is not what
                // the user asked for, and folding it into result_text would
                // put the thinking in the card's summary.
                _ => {}
            }
        }
        "turn.failed" | "error" => {
            let reason = payload(&v, "error")
                .and_then(|e| e.get("message").and_then(Value::as_str))
                .or_else(|| payload(&v, "message").and_then(Value::as_str))
                .unwrap_or("codex reported an error")
                .to_string();
            // Recorded rather than emitted: `finish` owns the terminal event,
            // so a run has exactly one, decided by the exit status.
            state.failure = Some(reason);
        }
        "turn.completed" => {}
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(lines: &[&str]) -> (Vec<AichipEvent>, StreamState) {
        let mut state = StreamState::new(Some("gpt-5-codex".into()));
        let mut out = vec![];
        for l in lines {
            out.extend(parse_line(l, &mut state));
        }
        (out, state)
    }

    #[test]
    fn reads_the_documented_dotted_spelling() {
        let (events, state) = drive(&[
            r#"{"type":"thread.started","thread_id":"th_1"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.completed","item":{"type":"agentMessage","id":"m1","text":"done"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":4}}"#,
        ]);
        assert_eq!(state.session_id.as_deref(), Some("th_1"));
        assert!(matches!(events[0], AichipEvent::RunStarted { .. }));
        assert!(events
            .iter()
            .any(|e| matches!(e, AichipEvent::AssistantText { text } if text == "done")));
        let AichipEvent::RunCompleted {
            result_text,
            usage,
            cost_usd,
            ..
        } = state.finish(true)
        else {
            panic!("expected completion")
        };
        assert_eq!(result_text, "done");
        assert_eq!(usage.output_tokens, 4);
        // Codex reports tokens, not money. Guessing a price would be worse
        // than saying nothing.
        assert_eq!(cost_usd, None);
    }

    #[test]
    fn reads_the_json_rpc_spelling_too() {
        // The docs show both; betting on one and being wrong would produce a
        // run with no events at all.
        let (events, state) = drive(&[
            r#"{"method":"thread/started","params":{"thread":{"id":"th_2"}}}"#,
            r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"m","text":"hi"}}}"#,
        ]);
        assert_eq!(state.session_id.as_deref(), Some("th_2"));
        assert!(events
            .iter()
            .any(|e| matches!(e, AichipEvent::AssistantText { text } if text == "hi")));
    }

    #[test]
    fn a_tool_call_becomes_a_call_and_a_result() {
        let (events, _) = drive(&[
            r#"{"type":"item.started","item":{"type":"commandExecution","id":"c1","name":"bash","command":"ls"}}"#,
            r#"{"type":"item.completed","item":{"type":"commandExecution","id":"c1","status":"failed","text":"no such file"}}"#,
        ]);
        assert!(
            matches!(&events[0], AichipEvent::ToolCall { tool_use_id, .. } if tool_use_id == "c1")
        );
        assert!(
            matches!(&events[1], AichipEvent::ToolResult { is_error, tool_use_id, .. }
                if *is_error && tool_use_id == "c1")
        );
    }

    #[test]
    fn a_started_message_does_not_duplicate_its_completed_text() {
        let (events, state) = drive(&[
            r#"{"type":"item.started","item":{"type":"agentMessage","id":"m","text":"partial"}}"#,
            r#"{"type":"item.completed","item":{"type":"agentMessage","id":"m","text":"partial and final"}}"#,
        ]);
        let texts: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AichipEvent::AssistantText { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["partial and final"]);
        let AichipEvent::RunCompleted { result_text, .. } = state.finish(true) else {
            panic!()
        };
        assert_eq!(result_text, "partial and final");
    }

    #[test]
    fn an_error_event_supplies_the_reason_and_finish_owns_the_verdict() {
        let (events, state) = drive(&[
            r#"{"type":"thread.started","thread_id":"t"}"#,
            r#"{"type":"turn.failed","error":{"message":"model refused"}}"#,
        ]);
        // Not emitted mid-stream: a run gets exactly one terminal event.
        assert!(!events
            .iter()
            .any(|e| matches!(e, AichipEvent::RunFailed { .. })));
        let AichipEvent::RunFailed { reason } = state.finish(true) else {
            panic!("a failure seen in the stream must beat a zero exit code")
        };
        assert_eq!(reason, "model refused");
    }

    #[test]
    fn a_nonzero_exit_fails_even_with_no_error_line() {
        let (_, state) = drive(&[r#"{"type":"thread.started","thread_id":"t"}"#]);
        assert!(matches!(state.finish(false), AichipEvent::RunFailed { .. }));
    }

    #[test]
    fn unknown_and_malformed_lines_are_survivable() {
        // The whole robustness story for an adapter written without the
        // binary: a shape it has never seen must thin the stream, not kill it.
        let (events, state) = drive(&[
            "not json at all",
            "",
            r#"{"type":"some.future.event","payload":{"whatever":1}}"#,
            r#"{"no_type_field":true}"#,
            r#"{"type":"item.completed","item":{"type":"agentMessage","id":"m","text":"still here"}}"#,
        ]);
        assert!(events
            .iter()
            .any(|e| matches!(e, AichipEvent::AssistantText { text } if text == "still here")));
        assert!(matches!(
            state.finish(true),
            AichipEvent::RunCompleted { .. }
        ));
    }
}
