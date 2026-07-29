//! The config aichip hands OpenCode for a single run.
//!
//! Delivered through `OPENCODE_CONFIG_CONTENT` rather than a file, for two
//! reasons found by reading OpenCode's own config loader:
//!
//! 1. Configs **merge**, and that variable is applied *after* the project's
//!    own `opencode.json`. So a repo containing `{"permission":{"bash":"allow"}}`
//!    cannot widen what aichip granted. The earlier `OPENCODE_CONFIG` slot is
//!    applied *before* project config and would lose that argument.
//! 2. There is no file to write, chmod, or garbage-collect, and two concurrent
//!    runs can't race over one path.
//!
//! The user's own global config still merges in underneath, so their providers,
//! themes and servers survive untouched.

use super::tools;
use crate::RunSpec;
use aichip_shared::{McpServerSpec, McpTransport, McpWiring, PermissionMode};
use serde_json::{json, Map, Value};
use std::path::Path;

/// The agent aichip defines and runs under.
pub const AGENT_NAME: &str = "aichip";

/// Build the whole config blob.
///
/// `instructions_path` carries the persona and recalled memory. It is passed
/// as a *file path under `instructions`* and deliberately **not** as
/// `agent.aichip.prompt`: OpenCode's prompt assembly is
/// `agent.prompt ? [agent.prompt] : baseProvider(model)`, so setting the agent
/// prompt would replace OpenCode's own coding instructions wholesale.
/// `instructions` is concatenated after the base prompt — the same semantics
/// as Claude's `--append-system-prompt`.
pub fn build(spec: &RunSpec, instructions_path: Option<&Path>) -> Value {
    let mut cfg = Map::new();
    cfg.insert("$schema".into(), json!("https://opencode.ai/config.json"));

    if let Some(path) = instructions_path {
        cfg.insert("instructions".into(), json!([path.to_string_lossy()]));
    }

    let mut agent = Map::new();
    agent.insert("mode".into(), json!("primary"));
    agent.insert("description".into(), json!("aichip-managed run"));
    if !spec.model_id.is_empty() {
        agent.insert("model".into(), json!(spec.model_id));
    }
    if let Some(permission) = permissions(spec) {
        agent.insert("permission".into(), permission);
    }
    cfg.insert("agent".into(), json!({ AGENT_NAME: Value::Object(agent) }));

    if let Some(mcp) = mcp(&spec.mcp) {
        cfg.insert("mcp".into(), mcp);
    }
    Value::Object(cfg)
}

/// Permission rules derived from the run's mode and allow-list.
///
/// `None` means "emit no rules", which leaves OpenCode on its own defaults.
/// That is the right answer for an empty allow-list: aichip's convention is
/// that an empty list means "don't restrict", and writing `{"*": "deny"}`
/// there would forbid everything instead.
fn permissions(spec: &RunSpec) -> Option<Value> {
    if spec.allowed_tools.is_empty() && spec.denied_tools.is_empty() {
        return None;
    }

    let mut rules = Map::new();
    // Closed by default, then opened per granted tool. Last match wins in
    // OpenCode, and insertion order is preserved by serde_json's map, so the
    // wildcard has to come first.
    rules.insert("*".into(), json!("deny"));

    for tool in &spec.allowed_tools {
        if let Some(key) = tools::permission_key(tool) {
            rules.insert(key.into(), json!("allow"));
        }
    }

    // Directories outside the worktree the run legitimately needs — the
    // analogue of Claude's `--add-dir`.
    if !spec.extra_read_dirs.is_empty() {
        let mut dirs = Map::new();
        dirs.insert("*".into(), json!("deny"));
        for dir in &spec.extra_read_dirs {
            dirs.insert(format!("{}/**", dir.to_string_lossy()), json!("allow"));
        }
        rules.insert("external_directory".into(), Value::Object(dirs));
    }

    // FullAuto passes `--auto`, which approves anything not explicitly
    // denied. The deny-by-default rule above still bites, so the two compose
    // rather than one overriding the other.
    if spec.permission_mode == PermissionMode::FullAuto {
        rules.insert("*".into(), json!("allow"));
    }

    // Denials go in last so they beat the allow-list *and* the FullAuto
    // wildcard above. A tool the caller said must not run must not run,
    // whatever else the run was granted.
    for tool in &spec.denied_tools {
        if let Some(key) = tools::permission_key(tool) {
            rules.insert(key.into(), json!("deny"));
        }
    }

    Some(Value::Object(rules))
}

/// aichip's endpoint plus the agent's servers, in OpenCode's dialect.
fn mcp(wiring: &McpWiring) -> Option<Value> {
    if wiring.is_empty() {
        return None;
    }
    let mut servers = Map::new();
    if let Some(url) = &wiring.aichip_url {
        servers.insert(
            "aichip".into(),
            json!({ "type": "remote", "url": url, "enabled": true }),
        );
    }
    for server in &wiring.servers {
        // aichip's own entry is never displaced — it is the channel approvals
        // and team messages travel through.
        if servers.contains_key(&server.name) {
            continue;
        }
        servers.insert(server.name.clone(), entry(server));
    }
    Some(Value::Object(servers))
}

fn entry(server: &McpServerSpec) -> Value {
    match &server.transport {
        McpTransport::Http { url, headers } => {
            let mut e = json!({ "type": "remote", "url": url, "enabled": true });
            if !headers.is_empty() {
                e["headers"] = pairs(headers);
            }
            e
        }
        McpTransport::Stdio { command, args, env } => {
            // OpenCode takes one `command` array, not a command plus args.
            let mut argv = vec![json!(command)];
            argv.extend(args.iter().map(|a| json!(a)));
            let mut e = json!({ "type": "local", "command": argv, "enabled": true });
            if !env.is_empty() {
                e["environment"] = pairs(env);
            }
            e
        }
    }
}

fn pairs(kv: &[(String, String)]) -> Value {
    Value::Object(kv.iter().map(|(k, v)| (k.clone(), json!(v))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aichip_shared::ModelTier;
    use std::path::PathBuf;

    fn spec() -> RunSpec {
        RunSpec {
            cwd: PathBuf::from("/tmp/wt"),
            prompt: "do it".into(),
            model_tier: ModelTier::Medium,
            model_id: "anthropic/claude-sonnet-4-5".into(),
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

    #[test]
    fn the_persona_goes_to_instructions_never_to_agent_prompt() {
        // Load-bearing, and the non-obvious part someone will try to
        // "simplify" later: OpenCode uses `agent.prompt ? [agent.prompt] :
        // baseProvider(model)`, so setting the agent prompt REPLACES its
        // built-in coding instructions. `instructions` appends instead.
        let cfg = build(&spec(), Some(Path::new("/home/u/.aichip/prompts/run-1.md")));
        assert_eq!(cfg["instructions"][0], "/home/u/.aichip/prompts/run-1.md");
        assert!(
            cfg["agent"][AGENT_NAME].get("prompt").is_none(),
            "setting agent.prompt would discard OpenCode's base prompt"
        );
    }

    #[test]
    fn an_empty_allow_list_writes_no_rules_at_all() {
        // aichip's convention: empty means "don't restrict". Emitting
        // `{"*":"deny"}` would invert that into "forbid everything".
        let cfg = build(&spec(), None);
        assert!(cfg["agent"][AGENT_NAME].get("permission").is_none());
    }

    #[test]
    fn granted_tools_become_allow_rules_over_a_deny_default() {
        let mut s = spec();
        s.allowed_tools = vec!["Read".into(), "Edit".into(), "Bash".into()];
        let p = &build(&s, None)["agent"][AGENT_NAME]["permission"];
        assert_eq!(p["*"], "deny");
        assert_eq!(p["read"], "allow");
        assert_eq!(p["edit"], "allow");
        assert_eq!(p["bash"], "allow");
        // Never granted, so never mentioned.
        assert!(p.get("websearch").is_none());
    }

    #[test]
    fn a_denial_beats_the_allow_list_and_the_full_auto_wildcard() {
        // Both orderings matter: an allow that came first must lose, and
        // FullAuto's `{"*": "allow"}` must not resurrect a denied tool.
        let mut s = spec();
        s.allowed_tools = vec!["Read".into(), "Bash".into(), "Edit".into()];
        s.denied_tools = vec!["Bash".into(), "Edit".into()];
        s.permission_mode = PermissionMode::FullAuto;
        let p = &build(&s, None)["agent"][AGENT_NAME]["permission"];
        assert_eq!(p["bash"], "deny");
        assert_eq!(p["edit"], "deny");
        assert_eq!(p["read"], "allow", "denying two tools must not close the rest");
    }

    #[test]
    fn denials_alone_still_produce_a_permission_block() {
        // Otherwise a read-only pass with no explicit allow-list would be
        // handed the empty-list "don't restrict" path and could write.
        let mut s = spec();
        s.allowed_tools = vec![];
        s.denied_tools = vec!["Edit".into()];
        let p = &build(&s, None)["agent"][AGENT_NAME]["permission"];
        assert_eq!(p["edit"], "deny");
    }

    #[test]
    fn mcp_tool_names_do_not_leak_into_permission_keys() {
        let mut s = spec();
        s.allowed_tools = vec!["Read".into(), "mcp__aichip__post_message".into()];
        let p = &build(&s, None)["agent"][AGENT_NAME]["permission"];
        assert_eq!(p["read"], "allow");
        assert_eq!(p.as_object().unwrap().len(), 2, "only `*` and `read`");
    }

    #[test]
    fn full_auto_opens_the_wildcard() {
        let mut s = spec();
        s.allowed_tools = vec!["Read".into()];
        s.permission_mode = PermissionMode::FullAuto;
        let p = &build(&s, None)["agent"][AGENT_NAME]["permission"];
        assert_eq!(p["*"], "allow");
    }

    #[test]
    fn extra_read_dirs_become_external_directory_rules() {
        let mut s = spec();
        s.allowed_tools = vec!["Read".into()];
        s.extra_read_dirs = vec![PathBuf::from("/home/u/.aichip/attachments/a1")];
        let p = &build(&s, None)["agent"][AGENT_NAME]["permission"];
        assert_eq!(p["external_directory"]["*"], "deny");
        assert_eq!(p["external_directory"]["/home/u/.aichip/attachments/a1/**"], "allow");
    }

    #[test]
    fn aichips_endpoint_becomes_a_remote_server() {
        let mut s = spec();
        s.mcp.aichip_url = Some("http://127.0.0.1:4820/mcp/run/x".into());
        let mcp = &build(&s, None)["mcp"];
        assert_eq!(mcp["aichip"]["type"], "remote");
        assert_eq!(mcp["aichip"]["url"], "http://127.0.0.1:4820/mcp/run/x");
    }

    #[test]
    fn a_stdio_server_becomes_one_command_array() {
        // OpenCode takes `command: [bin, ...args]`, not command + args.
        let mut s = spec();
        s.mcp.servers = vec![McpServerSpec {
            name: "playwright".into(),
            transport: McpTransport::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "@playwright/mcp".into()],
                env: vec![],
            },
        }];
        let e = &build(&s, None)["mcp"]["playwright"];
        assert_eq!(e["type"], "local");
        assert_eq!(e["command"], json!(["npx", "-y", "@playwright/mcp"]));
        assert!(e.get("environment").is_none(), "empty env is omitted");
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
        let mcp = &build(&s, None)["mcp"];
        assert_eq!(mcp["aichip"]["url"], "http://real");
        assert_eq!(mcp["aichip"]["type"], "remote");
    }

    #[test]
    fn no_wiring_means_no_mcp_key() {
        assert!(build(&spec(), None).get("mcp").is_none());
    }
}
