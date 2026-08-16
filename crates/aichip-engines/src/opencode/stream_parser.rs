//! `opencode run --format json` NDJSON → normalized [`AichipEvent`]s.
//!
//! Two things make this shaped differently from the Claude parser, both
//! observed by recording real runs (see `fixtures/`):
//!
//! * **There is no terminal event.** The stream simply ends. `RunCompleted`
//!   therefore cannot come from a line — it is synthesised on EOF, which is
//!   also when the exit code is known. Hence the explicit [`StreamState`]:
//!   the accumulation stays pure and fixture-testable instead of hiding in
//!   the adapter's tokio task.
//! * **A tool call arrives once, already finished.** One `tool_use` line
//!   carries input *and* output, so it fans out into a `ToolCall` and a
//!   `ToolResult` rather than mapping one-to-one.

use aichip_shared::{rate_limit_signal, AichipEvent, Usage};
use serde_json::Value;
use std::collections::BTreeMap;

/// Everything that must survive between lines to produce a terminal event.
#[derive(Debug, Default)]
pub struct StreamState {
    session_id: Option<String>,
    /// The model, which the stream never states — threaded in from the spec.
    model: Option<String>,
    /// `messageID` → that message's (cost, usage).
    ///
    /// Keyed and overwritten rather than summed, because a `step_finish`
    /// carries its message's running totals. Summing every line would
    /// multiply a multi-step message's cost by its step count.
    messages: BTreeMap<String, (f64, Usage)>,
    last_text: String,
    errors: Vec<String>,
    rate_limited: Option<(Option<String>, String)>,
}

impl StreamState {
    pub fn new(model: Option<String>) -> Self {
        Self {
            model,
            ..Default::default()
        }
    }

    /// Called on stdout EOF, with whether the process exited cleanly.
    pub fn finish(self, exit_ok: bool) -> AichipEvent {
        if let Some((_, message)) = self.rate_limited {
            return AichipEvent::RateLimited {
                reset_at: None,
                message,
            };
        }
        if !self.errors.is_empty() {
            return AichipEvent::RunFailed {
                reason: self.errors.join("\n"),
            };
        }
        if !exit_ok {
            return AichipEvent::RunFailed {
                reason: "opencode exited without completing the run".into(),
            };
        }
        let Some(session_id) = self.session_id else {
            return AichipEvent::RunFailed {
                reason: "opencode produced no session — it may not be authenticated".into(),
            };
        };
        let (cost, usage) =
            self.messages
                .values()
                .fold((0.0, Usage::default()), |(c, mut u), (mc, mu)| {
                    u.input_tokens += mu.input_tokens;
                    u.output_tokens += mu.output_tokens;
                    u.cache_read_tokens += mu.cache_read_tokens;
                    u.cache_creation_tokens += mu.cache_creation_tokens;
                    (c + mc, u)
                });
        AichipEvent::RunCompleted {
            session_id,
            cost_usd: Some(cost),
            usage,
            result_text: self.last_text,
        }
    }
}

/// One line of NDJSON. Non-JSON lines are tolerated: OpenCode prints update
/// banners and, notably, a plain-text permission notice (see below).
pub fn parse_line(line: &str, state: &mut StreamState) -> Vec<AichipEvent> {
    let line = line.trim();
    if line.is_empty() {
        return vec![];
    }

    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return parse_plain_text(line);
    };

    let mut events = vec![];

    // The session id is on every line and there is no init event, so the
    // first line carrying one is where a run "starts".
    if let Some(id) = v.get("sessionID").and_then(Value::as_str) {
        if state.session_id.is_none() {
            state.session_id = Some(id.to_string());
            events.push(AichipEvent::RunStarted {
                session_id: Some(id.to_string()),
                model: state.model.clone(),
            });
        }
    }

    let part = v.get("part").cloned().unwrap_or(Value::Null);
    match v.get("type").and_then(Value::as_str) {
        Some("text") => {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    state.last_text = text.to_string();
                    events.push(AichipEvent::AssistantText {
                        text: text.to_string(),
                    });
                }
            }
        }
        Some("tool_use") => events.extend(parse_tool(&part)),
        Some("step_finish") => {
            let usage = parse_usage(part.get("tokens").unwrap_or(&Value::Null));
            let cost = part.get("cost").and_then(Value::as_f64).unwrap_or(0.0);
            if let Some(id) = part.get("messageID").and_then(Value::as_str) {
                state.messages.insert(id.to_string(), (cost, usage.clone()));
            }
            events.push(AichipEvent::UsageUpdated { usage });
        }
        Some("error") => {
            let message = v
                .get("error")
                .map(|e| {
                    e.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| e.to_string())
                })
                .unwrap_or_else(|| "unknown error".into());
            if rate_limit_signal(&message) {
                state.rate_limited = Some((None, message));
            } else {
                // An error does not end the stream — OpenCode keeps going and
                // reflects it in the exit code, so this is recorded rather
                // than emitted as a terminal event.
                state.errors.push(message);
            }
        }
        // `step_start` is bookkeeping; `reasoning` only appears with
        // `--thinking`, which the adapter does not pass.
        _ => {}
    }
    events
}

/// A denied tool arrives as a plain-text line even in `--format json`:
/// `! permission requested: <tool> (<pattern>); auto-rejecting`.
///
/// Without this the run just looks like the model chose to do nothing, which
/// is the least debuggable outcome available.
fn parse_plain_text(line: &str) -> Vec<AichipEvent> {
    let Some(rest) = line.split_once("permission requested:").map(|(_, r)| r) else {
        return vec![];
    };
    let tool = rest
        .split(['(', ';'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    // Synthetic id: there is nothing to correlate with, and nothing to reply
    // to — the CLI has already rejected it.
    let request_id = format!("opencode-denied-{tool}");
    vec![
        AichipEvent::PermissionRequested {
            request_id: request_id.clone(),
            tool_name: tool,
            input: Value::Null,
        },
        AichipEvent::PermissionResolved {
            request_id,
            allowed: false,
        },
    ]
}

/// One OpenCode `tool_use` → a call *and* its result.
fn parse_tool(part: &Value) -> Vec<AichipEvent> {
    let tool_name = part
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let tool_use_id = part
        .get("callID")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let state = part.get("state").cloned().unwrap_or(Value::Null);

    let call = AichipEvent::ToolCall {
        tool_name,
        tool_use_id: tool_use_id.clone(),
        input: state.get("input").cloned().unwrap_or(Value::Null),
    };

    let is_error = state.get("status").and_then(Value::as_str) == Some("error");
    let summary = if is_error {
        state
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    } else {
        state
            .get("output")
            .and_then(Value::as_str)
            .or_else(|| state.get("title").and_then(Value::as_str))
            .unwrap_or("")
            .to_string()
    };

    vec![
        call,
        AichipEvent::ToolResult {
            tool_use_id,
            is_error,
            summary: clip(&summary, 400),
        },
    ]
}

fn parse_usage(t: &Value) -> Usage {
    let g = |k: &str| t.get(k).and_then(Value::as_u64).unwrap_or(0);
    let cache = t.get("cache").cloned().unwrap_or(Value::Null);
    let c = |k: &str| cache.get(k).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input_tokens: g("input"),
        output_tokens: g("output"),
        cache_read_tokens: c("read"),
        cache_creation_tokens: c("write"),
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from opencode 1.18.9 — see `fixtures/README.md`.
    const SIMPLE: &str = include_str!("fixtures/simple_text.ndjson");
    const TOOLS: &str = include_str!("fixtures/tool_and_two_steps.ndjson");

    fn run(fixture: &str, exit_ok: bool) -> (Vec<AichipEvent>, AichipEvent) {
        let mut state = StreamState::new(Some("google/gemini-3.6-flash".into()));
        let mut events = vec![];
        for line in fixture.lines() {
            events.extend(parse_line(line, &mut state));
        }
        let terminal = state.finish(exit_ok);
        (events, terminal)
    }

    #[test]
    fn a_session_starts_on_the_first_line_that_names_one() {
        let (events, _) = run(SIMPLE, true);
        let starts: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, AichipEvent::RunStarted { .. }))
            .collect();
        assert_eq!(
            starts.len(),
            1,
            "exactly one start, however many lines carry the id"
        );
        match starts[0] {
            AichipEvent::RunStarted { session_id, model } => {
                assert!(session_id.as_deref().unwrap().starts_with("ses_"));
                assert_eq!(model.as_deref(), Some("google/gemini-3.6-flash"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn the_terminal_event_is_synthesised_because_the_stream_has_none() {
        let (events, terminal) = run(SIMPLE, true);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AichipEvent::RunCompleted { .. })),
            "no line should produce a completion — OpenCode emits none"
        );
        match terminal {
            AichipEvent::RunCompleted {
                result_text,
                cost_usd,
                ..
            } => {
                assert_eq!(result_text, "ok");
                assert!(cost_usd.unwrap() > 0.0);
            }
            other => panic!("expected RunCompleted, got {other:?}"),
        }
    }

    #[test]
    fn one_tool_line_becomes_a_call_and_a_result() {
        let (events, _) = run(TOOLS, true);
        let calls: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AichipEvent::ToolCall {
                    tool_name,
                    tool_use_id,
                    ..
                } => Some((tool_name.clone(), tool_use_id.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "read");

        let results: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AichipEvent::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } => Some((tool_use_id.clone(), *is_error)),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 1);
        assert!(!results[0].1);
        assert_eq!(results[0].0, calls[0].1, "call and result must correlate");
    }

    #[test]
    fn cost_sums_across_messages_without_double_counting() {
        // The recorded run has two steps under two message ids costing
        // 0.0152055 and 0.0140955. Note the second is *cheaper* — which is
        // how we know a step_finish carries its own message's cost rather
        // than a running total for the whole run.
        let (_, terminal) = run(TOOLS, true);
        match terminal {
            AichipEvent::RunCompleted {
                cost_usd, usage, ..
            } => {
                let cost = cost_usd.unwrap();
                assert!((cost - 0.029301).abs() < 1e-6, "got {cost}");
                assert_eq!(usage.input_tokens, 8747 + 9152);
                assert_eq!(usage.output_tokens, 81 + 2);
            }
            other => panic!("expected RunCompleted, got {other:?}"),
        }
    }

    #[test]
    fn a_nonzero_exit_fails_the_run_even_with_a_clean_stream() {
        let (_, terminal) = run(SIMPLE, false);
        assert!(matches!(terminal, AichipEvent::RunFailed { .. }));
    }

    #[test]
    fn an_error_line_is_recorded_but_does_not_end_the_stream() {
        let mut state = StreamState::new(None);
        let events = parse_line(
            r#"{"type":"error","sessionID":"ses_1","error":"tool blew up"}"#,
            &mut state,
        );
        // Only the synthesised start; the error itself waits for the end.
        assert!(events
            .iter()
            .all(|e| matches!(e, AichipEvent::RunStarted { .. })));
        match state.finish(false) {
            AichipEvent::RunFailed { reason } => assert!(reason.contains("tool blew up")),
            other => panic!("expected RunFailed, got {other:?}"),
        }
    }

    #[test]
    fn a_throttling_error_becomes_a_rate_limit_rather_than_a_failure() {
        let mut state = StreamState::new(None);
        parse_line(
            r#"{"type":"error","sessionID":"ses_1","error":"RESOURCE_EXHAUSTED: quota exceeded"}"#,
            &mut state,
        );
        match state.finish(false) {
            AichipEvent::RateLimited { reset_at, message } => {
                // OpenCode has no structured reset time — see Capabilities.
                assert!(reset_at.is_none());
                assert!(message.contains("quota"));
            }
            other => panic!("expected RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn a_denied_tool_is_visible_instead_of_looking_like_inaction() {
        // Plain text, even in --format json. Without parsing it, a run that
        // was blocked looks identical to one that decided to do nothing.
        let mut state = StreamState::new(None);
        let events = parse_line(
            "!  permission requested: bash (rm *); auto-rejecting",
            &mut state,
        );
        assert_eq!(events.len(), 2);
        match &events[0] {
            AichipEvent::PermissionRequested { tool_name, .. } => assert_eq!(tool_name, "bash"),
            other => panic!("expected PermissionRequested, got {other:?}"),
        }
        assert!(matches!(
            events[1],
            AichipEvent::PermissionResolved { allowed: false, .. }
        ));
    }

    #[test]
    fn noise_is_ignored_without_panicking() {
        let mut state = StreamState::new(None);
        for line in ["", "   ", "Update available!", "{not json", "{}"] {
            assert!(parse_line(line, &mut state).is_empty(), "line: {line:?}");
        }
    }

    #[test]
    fn a_stream_that_never_names_a_session_fails_with_a_useful_reason() {
        let state = StreamState::new(None);
        match state.finish(true) {
            AichipEvent::RunFailed { reason } => assert!(reason.contains("authenticated")),
            other => panic!("expected RunFailed, got {other:?}"),
        }
    }
}
