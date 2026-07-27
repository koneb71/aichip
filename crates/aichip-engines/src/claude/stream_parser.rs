//! Pure parser: one line of `claude --output-format stream-json` output →
//! zero or more normalized [`AichipEvent`]s. Pure so it is trivially
//! fixture-testable; all process I/O lives in the adapter.

use aichip_shared::{AichipEvent, Usage};
use serde_json::Value;

pub fn parse_line(line: &str) -> Vec<AichipEvent> {
    let line = line.trim();
    if line.is_empty() {
        return vec![];
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        // Non-JSON noise on stdout (progress spinners, warnings) is ignored;
        // the adapter separately watches stderr for fatal errors.
        return vec![];
    };
    match v.get("type").and_then(Value::as_str) {
        Some("system") => parse_system(&v),
        Some("assistant") => parse_assistant(&v),
        Some("user") => parse_user(&v),
        Some("result") => parse_result(&v),
        Some("rate_limit_event") => parse_rate_limit_event(&v),
        _ => vec![],
    }
}

/// The CLI emits structured rate-limit telemetry (observed in v2.1.x):
/// `{"type":"rate_limit_event","rate_limit_info":{"status":"allowed",
///   "resetsAt":1785183600,"rateLimitType":"five_hour",...}}`.
/// `status:"allowed"` is routine telemetry; anything else means the run is
/// being throttled and the queue should back off until `resetsAt`.
fn parse_rate_limit_event(v: &Value) -> Vec<AichipEvent> {
    let info = v.get("rate_limit_info").cloned().unwrap_or(Value::Null);
    let status = info.get("status").and_then(Value::as_str).unwrap_or("allowed");
    if status == "allowed" {
        return vec![];
    }
    let reset_at = info
        .get("resetsAt")
        .and_then(Value::as_i64)
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0));
    let limit_type = info
        .get("rateLimitType")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    vec![AichipEvent::RateLimited {
        reset_at,
        message: format!("rate limit ({limit_type}) status: {status}"),
    }]
}

fn parse_system(v: &Value) -> Vec<AichipEvent> {
    if v.get("subtype").and_then(Value::as_str) == Some("init") {
        vec![AichipEvent::RunStarted {
            session_id: v.get("session_id").and_then(Value::as_str).map(String::from),
            model: v.get("model").and_then(Value::as_str).map(String::from),
        }]
    } else {
        vec![]
    }
}

fn parse_assistant(v: &Value) -> Vec<AichipEvent> {
    let mut events = vec![];
    let content = v
        .pointer("/message/content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for block in &content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        events.push(AichipEvent::AssistantText {
                            text: text.to_string(),
                        });
                    }
                }
            }
            Some("tool_use") => {
                events.push(AichipEvent::ToolCall {
                    tool_name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                        .to_string(),
                    tool_use_id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    input: block.get("input").cloned().unwrap_or(Value::Null),
                });
            }
            _ => {}
        }
    }
    if let Some(usage) = v.pointer("/message/usage") {
        events.push(AichipEvent::UsageUpdated {
            usage: parse_usage(usage),
        });
    }
    events
}

fn parse_user(v: &Value) -> Vec<AichipEvent> {
    let mut events = vec![];
    let content = v
        .pointer("/message/content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for block in &content {
        if block.get("type").and_then(Value::as_str) == Some("tool_result") {
            events.push(AichipEvent::ToolResult {
                tool_use_id: block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                is_error: block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                summary: summarize_tool_result(block),
            });
        }
    }
    events
}

fn parse_result(v: &Value) -> Vec<AichipEvent> {
    let subtype = v.get("subtype").and_then(Value::as_str).unwrap_or("");
    let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    let result_text = v
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if rate_limit_signal(&result_text) || rate_limit_signal(subtype) {
        return vec![AichipEvent::RateLimited {
            reset_at: None,
            message: result_text,
        }];
    }
    if is_error || subtype.starts_with("error") {
        return vec![AichipEvent::RunFailed {
            reason: if result_text.is_empty() {
                format!("engine reported error ({subtype})")
            } else {
                result_text
            },
        }];
    }
    vec![AichipEvent::RunCompleted {
        session_id: v
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
        usage: v.get("usage").map(parse_usage).unwrap_or_default(),
        result_text,
    }]
}

fn parse_usage(u: &Value) -> Usage {
    let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input_tokens: g("input_tokens"),
        output_tokens: g("output_tokens"),
        cache_read_tokens: g("cache_read_input_tokens"),
        cache_creation_tokens: g("cache_creation_input_tokens"),
    }
}

fn summarize_tool_result(block: &Value) -> String {
    const MAX: usize = 400;
    let text = match block.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    let mut out: String = text.chars().take(MAX).collect();
    if text.chars().count() > MAX {
        out.push('…');
    }
    out
}

pub fn rate_limit_signal(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("rate limit")
        || t.contains("rate_limit")
        || t.contains("usage limit")
        || t.contains("overloaded")
        || t.contains("429")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_line_yields_run_started() {
        let line = r#"{"type":"system","subtype":"init","session_id":"abc-123","model":"claude-opus-5","tools":["Bash"]}"#;
        let events = parse_line(line);
        assert_eq!(
            events,
            vec![AichipEvent::RunStarted {
                session_id: Some("abc-123".into()),
                model: Some("claude-opus-5".into()),
            }]
        );
    }

    #[test]
    fn assistant_text_and_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"On it."},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":10,"output_tokens":5}}}"#;
        let events = parse_line(line);
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[0], AichipEvent::AssistantText { text } if text == "On it."));
        assert!(
            matches!(&events[1], AichipEvent::ToolCall { tool_name, .. } if tool_name == "Bash")
        );
        assert!(matches!(
            &events[2],
            AichipEvent::UsageUpdated { usage } if usage.input_tokens == 10 && usage.output_tokens == 5
        ));
    }

    #[test]
    fn tool_result_is_summarized() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":false,"content":"file1\nfile2"}]}}"#;
        let events = parse_line(line);
        assert_eq!(
            events,
            vec![AichipEvent::ToolResult {
                tool_use_id: "toolu_1".into(),
                is_error: false,
                summary: "file1\nfile2".into(),
            }]
        );
    }

    #[test]
    fn success_result_yields_run_completed() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"Done.","session_id":"abc-123","total_cost_usd":0.042,"usage":{"input_tokens":100,"output_tokens":50}}"#;
        let events = parse_line(line);
        match &events[..] {
            [AichipEvent::RunCompleted {
                session_id,
                cost_usd,
                usage,
                result_text,
            }] => {
                assert_eq!(session_id, "abc-123");
                assert_eq!(*cost_usd, Some(0.042));
                assert_eq!(usage.input_tokens, 100);
                assert_eq!(result_text, "Done.");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn error_result_yields_run_failed() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"boom"}"#;
        let events = parse_line(line);
        assert!(matches!(&events[..], [AichipEvent::RunFailed { reason }] if reason == "boom"));
    }

    #[test]
    fn rate_limit_result_is_detected() {
        let line = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"You have hit your usage limit. Limit resets at 3pm."}"#;
        let events = parse_line(line);
        assert!(matches!(&events[..], [AichipEvent::RateLimited { .. }]));
    }

    #[test]
    fn rate_limit_event_allowed_is_telemetry_only() {
        // Shape recorded from claude CLI 2.1.205 on 2026-07-28.
        let line = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1785183600,"rateLimitType":"five_hour","overageStatus":"rejected","isUsingOverage":false},"session_id":"s1"}"#;
        assert!(parse_line(line).is_empty());
    }

    #[test]
    fn rate_limit_event_blocked_carries_reset_time() {
        let line = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1785183600,"rateLimitType":"five_hour"},"session_id":"s1"}"#;
        let events = parse_line(line);
        match &events[..] {
            [AichipEvent::RateLimited { reset_at, message }] => {
                assert_eq!(reset_at.unwrap().timestamp(), 1785183600);
                assert!(message.contains("five_hour"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn malformed_and_noise_lines_are_ignored() {
        assert!(parse_line("").is_empty());
        assert!(parse_line("not json at all").is_empty());
        assert!(parse_line(r#"{"type":"unknown_thing"}"#).is_empty());
        assert!(parse_line(r#"{"no_type":true}"#).is_empty());
    }
}
