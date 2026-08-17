//! What aichip tells Codex about one run, as `-c key=value` overrides.
//!
//! Delivered on the command line rather than through a config file, and that
//! is not a convenience — the second compliance invariant forbids aichip from
//! touching `~/.codex/config.toml` at all. `codex mcp add` would write to it;
//! `-c` is the documented highest-precedence layer and needs no file to
//! create, chmod, or garbage-collect, and two concurrent runs cannot race over
//! one path.
//!
//! `-c` **deep-merges** into whatever the user's own config says rather than
//! replacing it, which is mostly what you want — their model provider and
//! their own servers survive — with one narrow hazard worth stating: a user
//! who has defined an MCP server of their own literally named `aichip` would
//! get a hybrid of theirs and ours rather than ours. aichip cannot detect that
//! without reading their config, which it will not do.
//!
//! Everything here was checked against `codex-cli 0.147.0` on the machine
//! where it was written. Where a value could not be verified end to end, the
//! comment says so rather than the code implying otherwise.

use crate::RunSpec;
use aichip_shared::{McpTransport, McpWiring, PermissionMode, ReasoningEffort};

/// Tools whose denial means "this run must not change anything".
///
/// aichip expresses "read-only" as a denial list, because that is the
/// vocabulary Claude Code and OpenCode share. Codex has no per-tool permission
/// vocabulary at all — its lever is the sandbox — so the translation is: if
/// the caller denied any tool that could write, the sandbox must be one where
/// nothing can.
const WRITE_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit", "Bash"];

/// The sandbox this run gets.
///
/// The ordering is the whole point, and it is the same rule the OpenCode
/// adapter applies to its permission map: **a denial beats the permission
/// mode, including `FullAuto`.**
///
/// Getting that backwards is not a cosmetic bug. A chat run is dispatched
/// `FullAuto` on purpose — see `CHAT_PERMISSION` in the orchestrator — and the
/// justification written there is "'everything' here is bounded by the
/// denials". Chat runs execute in the user's **real checkout**, not a
/// worktree. So a Codex chat that honoured `FullAuto` and dropped
/// `CHAT_DENIED_TOOLS` would be handed `danger-full-access` over the working
/// tree with no approval prompt and nothing to undo from — a privilege
/// escalation performed on the user's behalf, which is exactly what the
/// capability system exists to prevent.
pub fn sandbox_mode(spec: &RunSpec) -> &'static str {
    if spec
        .denied_tools
        .iter()
        .any(|t| WRITE_TOOLS.iter().any(|w| w.eq_ignore_ascii_case(t)))
    {
        return "read-only";
    }
    match spec.permission_mode {
        PermissionMode::FullAuto => "danger-full-access",
        // Reviewed never reaches here — `vet` refuses it, because Codex has no
        // way to stop and ask. Treating it as the containing sandbox anyway is
        // defence in depth, not a downgrade.
        _ => "workspace-write",
    }
}

/// aichip's effort scale in Codex's vocabulary.
///
/// `xhigh` is real and was verified to take effect (a run at the default
/// reported 0 reasoning tokens; the same prompt at `xhigh` reported 45).
///
/// `Max` deliberately does **not** map to `"max"`. The value exists in the
/// binary's own enum, but OpenAI's config reference documents the scale as
/// ending at `xhigh` and calls even that model-dependent — and an effort a
/// model does not accept is a config error at spawn, i.e. a run that never
/// starts. Mapping down one verified notch is a smaller lie than a run that
/// dies. The previous mapping collapsed both XHigh and Max all the way to
/// `high`, which is the thing actually worth fixing.
pub fn effort(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh | ReasoningEffort::Max => "xhigh",
    }
}

/// Every `-c key=value` this run needs, in order.
///
/// One flag per key rather than one nested table: the dotted form keeps
/// Codex's error messages pointing at the offending key, and avoids building
/// nested TOML by string concatenation.
pub fn overrides(spec: &RunSpec) -> anyhow::Result<Vec<String>> {
    let mut out = vec![
        // Stated rather than relied upon. `codex exec` already defaults to
        // never asking, but the user's own config.toml participates in every
        // run, and one that set `approval_policy = "on-request"` would leave a
        // headless run waiting for a console that does not exist.
        r#"approval_policy="never""#.to_string(),
        // Passed as config rather than as `--sandbox`, because `codex exec
        // resume` rejects `--sandbox` outright. One spelling that works on
        // both paths beats two that each work on one.
        format!(r#"sandbox_mode="{}""#, sandbox_mode(spec)),
    ];

    if let Some(e) = spec.effort {
        out.push(format!(r#"model_reasoning_effort="{}""#, effort(e)));
    }

    // The persona and recalled memory. Verified additive, not replacing: with
    // this set, a run still knew its own sandbox mode and still used its shell
    // tool correctly, so Codex's own instructions survive alongside it. That
    // is the distinction `opencode::config` documents at length for
    // `agent.prompt`, and the reason this is not `base_instructions`.
    if let Some(body) = spec
        .append_system_prompt
        .as_deref()
        .filter(|p| !p.is_empty())
    {
        out.push(format!("developer_instructions={}", toml_string(body)));
    }

    // Directories outside the worktree the run legitimately needs — today the
    // per-attachment dirs. Codex's only lever here makes them *writable*,
    // which is wider than "readable"; it is still the honest declaration,
    // and under `read-only` it grants nothing at all. `--add-dir` says the
    // same thing but exists only on the fresh path, not on resume.
    if !spec.extra_read_dirs.is_empty() && sandbox_mode(spec) != "read-only" {
        let roots: Vec<String> = spec
            .extra_read_dirs
            .iter()
            .map(|d| toml_string(&d.to_string_lossy()))
            .collect();
        out.push(format!(
            "sandbox_workspace_write.writable_roots=[{}]",
            roots.join(", ")
        ));
    }

    out.extend(mcp(&spec.mcp)?);
    Ok(out)
}

/// aichip's endpoint plus the agent's own servers, in Codex's dialect.
///
/// Transport is discriminated by which key is present — `url` for streamable
/// HTTP, `command` for stdio — and Codex hard-fails at startup if a server
/// carries both, so exactly one shape is emitted per server.
fn mcp(wiring: &McpWiring) -> anyhow::Result<Vec<String>> {
    let mut out = vec![];
    if let Some(url) = &wiring.aichip_url {
        out.push(format!("mcp_servers.aichip.url={}", toml_string(url)));
        // Codex marks a server it could not reach as merely absent, so a
        // broken wiring would present as an agent that has quietly lost its
        // tools. Fail the run instead.
        out.push("mcp_servers.aichip.required=true".to_string());
        // Without this the tools are *visible and uncallable*. Codex lists
        // them, the model calls one, and under `approval_policy = "never"` the
        // call is rejected locally in under a millisecond with
        // `"user cancelled MCP tool call"` — no request ever reaches aichip.
        // The agent then truthfully reports that it could not find out, which
        // reads exactly like a broken server. This is what openai/codex#29857
        // describes, and it is the difference between the wiring existing and
        // the wiring working.
        //
        // The value is `approve`, and it has to be exactly that. The mode is a
        // closed enum of `prompt | writes | approve`; `writes` still cancels a
        // read-only call, and an unrecognised value — `auto`, say — is
        // *silently ignored*, because Codex drops `-c` keys and values it does
        // not know rather than complaining. So a typo here is invisible and
        // presents as the original bug. Verified by running all three.
        out.push("mcp_servers.aichip.default_tools_approval_mode=\"approve\"".to_string());
        // The default is 60 seconds, and one of these tools is `ask_user`,
        // which by design waits for a person. Fifteen minutes matches the
        // permission broker's own window, so the decision to give up stays
        // with aichip rather than being made twice with different answers.
        out.push("mcp_servers.aichip.tool_timeout_sec=900".to_string());
    }

    for server in &wiring.servers {
        // aichip's own entry is the channel approvals and board tools travel
        // through, and nothing the user configured may displace it.
        let key = server_key(&server.name);
        if key == "aichip" {
            continue;
        }
        match &server.transport {
            McpTransport::Http { url, headers } => {
                if !headers.is_empty() {
                    // The only way to give Codex a header is on its command
                    // line, where `ps` shows it to every process on the
                    // machine. A header on an MCP server is virtually always a
                    // credential, and there is no key here to test — so this
                    // refuses rather than leaking, in the same spirit as
                    // `auth_env_refusal`.
                    anyhow::bail!(
                        "\"{}\" sends HTTP headers, and Codex can only be given those on its \
                         command line where any process on this machine could read them. \
                         aichip won't put a credential there. Run this on Claude Code or \
                         OpenCode, or remove the headers from that server.",
                        server.name
                    );
                }
                out.push(format!("mcp_servers.{key}.url={}", toml_string(url)));
            }
            McpTransport::Stdio { command, args, env } => {
                out.push(format!(
                    "mcp_servers.{key}.command={}",
                    toml_string(command)
                ));
                if !args.is_empty() {
                    let items: Vec<String> = args.iter().map(|a| toml_string(a)).collect();
                    out.push(format!("mcp_servers.{key}.args=[{}]", items.join(", ")));
                }
                for (k, v) in env {
                    // Already refused where these are stored, and refused
                    // again here: this value lands on argv, and the guard that
                    // matters is the one closest to the process.
                    if aichip_shared::is_auth_env(k) {
                        anyhow::bail!("{}", aichip_shared::auth_env_refusal(k));
                    }
                    out.push(format!("mcp_servers.{key}.env.{k}={}", toml_string(v)));
                }
            }
        }
    }
    Ok(out)
}

/// A server name Codex will accept as a TOML bare key.
///
/// A dot breaks the dotted path this is interpolated into — the key stops
/// being one key — and quoting does not rescue it. Anything outside the bare
/// set becomes an underscore rather than being dropped, so two different names
/// stay two different servers.
fn server_key(name: &str) -> String {
    let key: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if key.is_empty() {
        "server".to_string()
    } else {
        key
    }
}

/// A TOML basic string, quotes included.
///
/// The value half of `-c` falls back to a raw literal when it does not parse
/// as TOML, which is a silent trap for anything that happens to parse as
/// something else — so everything is quoted explicitly.
fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use aichip_shared::{McpServerSpec, ModelTier};
    use std::path::PathBuf;

    pub(super) fn spec() -> RunSpec {
        RunSpec {
            cwd: PathBuf::from("/tmp/wt"),
            prompt: "do it".into(),
            model_tier: ModelTier::Medium,
            model_id: "gpt-5.5".into(),
            effort: None,
            resume_session_id: None,
            permission_mode: PermissionMode::AutoEdit,
            allowed_tools: vec![],
            denied_tools: vec![],
            append_system_prompt: None,
            mcp: McpWiring::default(),
            run_key: "run-1".into(),
            extra_read_dirs: vec![],
            permission_prompt_tool: false,
            extra_env: Default::default(),
        }
    }

    fn has(v: &[String], needle: &str) -> bool {
        v.iter().any(|s| s == needle)
    }

    #[test]
    fn a_denial_beats_full_auto_rather_than_the_other_way_round() {
        // The load-bearing test in this file. A chat run is dispatched
        // FullAuto *because* its denials bound it, and it stands in the user's
        // real checkout — so if FullAuto won here, a chat message could delete
        // the working tree with nothing to undo from.
        let mut s = spec();
        s.permission_mode = PermissionMode::FullAuto;
        s.denied_tools = vec!["Edit".into(), "Write".into(), "Bash".into()];
        assert_eq!(sandbox_mode(&s), "read-only");
        assert!(has(&overrides(&s).unwrap(), r#"sandbox_mode="read-only""#));
    }

    #[test]
    fn full_auto_without_denials_still_gets_the_wide_sandbox() {
        let mut s = spec();
        s.permission_mode = PermissionMode::FullAuto;
        assert_eq!(sandbox_mode(&s), "danger-full-access");
    }

    #[test]
    fn an_ordinary_card_is_contained_but_can_still_work() {
        assert_eq!(sandbox_mode(&spec()), "workspace-write");
        // Denying something that cannot write must not close the sandbox — a
        // run that denies WebFetch is still allowed to edit code.
        let mut s = spec();
        s.denied_tools = vec!["WebFetch".into(), "WebSearch".into()];
        assert_eq!(sandbox_mode(&s), "workspace-write");
    }

    #[test]
    fn the_denial_test_is_not_case_sensitive() {
        // `denied_tools` is free text from several call sites; a spelling
        // difference must not silently widen the sandbox.
        let mut s = spec();
        s.denied_tools = vec!["bash".into()];
        assert_eq!(sandbox_mode(&s), "read-only");
    }

    #[test]
    fn approval_is_stated_rather_than_assumed() {
        // `codex exec` defaults to never asking, but the user's own
        // config.toml merges in underneath and could say otherwise, which
        // headless means a run that waits forever.
        assert!(has(
            &overrides(&spec()).unwrap(),
            r#"approval_policy="never""#
        ));
    }

    #[test]
    fn effort_uses_the_highest_level_that_was_actually_verified() {
        let mut s = spec();
        s.effort = Some(ReasoningEffort::XHigh);
        assert!(has(
            &overrides(&s).unwrap(),
            r#"model_reasoning_effort="xhigh""#
        ));
        // Max maps to xhigh rather than to "max": see `effort`.
        s.effort = Some(ReasoningEffort::Max);
        assert_eq!(effort(ReasoningEffort::Max), "xhigh");
        // And no effort at all pins nothing.
        s.effort = None;
        assert!(!overrides(&s)
            .unwrap()
            .iter()
            .any(|o| o.starts_with("model_reasoning_effort")));
    }

    #[test]
    fn the_persona_reaches_the_model_and_is_quoted_safely() {
        let mut s = spec();
        s.append_system_prompt = Some("You are \"Ada\".\nBe terse.\\done".into());
        let o = overrides(&s).unwrap();
        let line = o
            .iter()
            .find(|l| l.starts_with("developer_instructions="))
            .unwrap();
        // Quotes, newlines and backslashes all escaped — an unescaped one
        // would end the TOML string early and the rest would be read as config.
        assert_eq!(
            line,
            r#"developer_instructions="You are \"Ada\".\nBe terse.\\done""#
        );
    }

    #[test]
    fn aichips_endpoint_is_declared_and_made_load_bearing() {
        let mut s = spec();
        s.mcp.aichip_url = Some("http://127.0.0.1:4820/mcp/run/x".into());
        let o = overrides(&s).unwrap();
        assert!(has(
            &o,
            r#"mcp_servers.aichip.url="http://127.0.0.1:4820/mcp/run/x""#
        ));
        // Without this a server Codex could not reach is simply absent, and
        // the run presents as an agent that quietly lost its tools.
        assert!(has(&o, "mcp_servers.aichip.required=true"));
        // And without this its tools are visible but uncallable — the call
        // is rejected locally with "user cancelled MCP tool call" before any
        // request is made. The value is a closed enum and an unknown one is
        // silently ignored, so `approve` is pinned rather than described.
        assert!(has(
            &o,
            r#"mcp_servers.aichip.default_tools_approval_mode="approve""#
        ));
        // `ask_user` waits for a person; the 60-second default would give up
        // long before they answered.
        assert!(has(&o, "mcp_servers.aichip.tool_timeout_sec=900"));
    }

    #[test]
    fn a_user_server_named_aichip_cannot_shadow_the_proxy() {
        let mut s = spec();
        s.mcp.aichip_url = Some("http://real".into());
        s.mcp.servers = vec![McpServerSpec {
            name: "aichip".into(),
            transport: McpTransport::Stdio {
                command: "evil".into(),
                args: vec![],
                env: vec![],
            },
        }];
        let o = overrides(&s).unwrap();
        assert!(has(&o, r#"mcp_servers.aichip.url="http://real""#));
        assert!(!o.iter().any(|l| l.contains("evil")));
    }

    #[test]
    fn a_stdio_server_becomes_command_and_args() {
        let mut s = spec();
        s.mcp.servers = vec![McpServerSpec {
            name: "playwright".into(),
            transport: McpTransport::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "@playwright/mcp".into()],
                env: vec![("PW_HEADLESS".into(), "1".into())],
            },
        }];
        let o = overrides(&s).unwrap();
        assert!(has(&o, r#"mcp_servers.playwright.command="npx""#));
        assert!(has(
            &o,
            r#"mcp_servers.playwright.args=["-y", "@playwright/mcp"]"#
        ));
        assert!(has(&o, r#"mcp_servers.playwright.env.PW_HEADLESS="1""#));
    }

    #[test]
    fn a_name_that_is_not_a_toml_key_is_made_into_one() {
        // A dot would stop the dotted path being one key, and quoting does not
        // rescue it — the whole config fails to load.
        assert_eq!(server_key("my.server"), "my_server");
        assert_eq!(server_key("a b/c"), "a_b_c");
        assert_eq!(server_key(""), "server");
        // Already-valid names are left exactly alone, including a uuid.
        assert_eq!(server_key("play-wright_2"), "play-wright_2");
    }

    #[test]
    fn a_credential_is_refused_rather_than_put_on_the_command_line() {
        // Two shapes, one rule: anything that would land on argv, where `ps`
        // shows it to every process on the machine, is refused.
        let mut s = spec();
        s.mcp.servers = vec![McpServerSpec {
            name: "vendor".into(),
            transport: McpTransport::Http {
                url: "https://mcp.example.com".into(),
                headers: vec![("Authorization".into(), "Bearer sk-live-abc".into())],
            },
        }];
        let err = overrides(&s).unwrap_err().to_string();
        assert!(err.contains("vendor"), "the message must name the server");
        assert!(err.contains("OpenCode"), "and offer a way forward");
        assert!(!err.contains("sk-live-abc"), "and not repeat the secret");

        let mut s = spec();
        s.mcp.servers = vec![McpServerSpec {
            name: "vendor".into(),
            transport: McpTransport::Stdio {
                command: "srv".into(),
                args: vec![],
                env: vec![("VENDOR_API_KEY".into(), "sk-live-abc".into())],
            },
        }];
        assert!(overrides(&s).is_err());
    }

    #[test]
    fn a_header_free_http_server_is_fine() {
        let mut s = spec();
        s.mcp.servers = vec![McpServerSpec {
            name: "docs".into(),
            transport: McpTransport::Http {
                url: "https://mcp.example.com/x".into(),
                headers: vec![],
            },
        }];
        assert!(has(
            &overrides(&s).unwrap(),
            r#"mcp_servers.docs.url="https://mcp.example.com/x""#
        ));
    }

    #[test]
    fn attachments_are_declared_only_where_the_declaration_means_something() {
        let mut s = spec();
        s.extra_read_dirs = vec![PathBuf::from("/home/u/.aichip/attachments/a1")];
        assert!(has(
            &overrides(&s).unwrap(),
            r#"sandbox_workspace_write.writable_roots=["/home/u/.aichip/attachments/a1"]"#
        ));
        // Under read-only it grants nothing, so saying it would be noise.
        s.denied_tools = vec!["Write".into()];
        assert!(!overrides(&s)
            .unwrap()
            .iter()
            .any(|o| o.starts_with("sandbox_workspace_write")));
    }

    #[test]
    fn no_wiring_mentions_no_servers() {
        assert!(!overrides(&spec())
            .unwrap()
            .iter()
            .any(|o| o.starts_with("mcp_servers")));
    }
}
