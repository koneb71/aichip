//! Rendering [`McpWiring`] into Claude Code's `--mcp-config` file.
//!
//! This moved out of the orchestrator, which used to write this dialect
//! directly and therefore had to check `engine_id == "claude-code"` before
//! doing so. Knowing one CLI's file format is the adapter's job.

use aichip_shared::{McpServerSpec, McpTransport, McpWiring};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// The `mcpServers` object Claude Code expects.
///
/// aichip's own entry is written first and cannot be displaced: a user server
/// that happened to be called `aichip` would otherwise shadow the permission
/// proxy — the channel every approval prompt travels through.
pub fn config(wiring: &McpWiring) -> Value {
    let mut servers = Map::new();
    if let Some(url) = &wiring.aichip_url {
        servers.insert("aichip".into(), json!({ "type": "http", "url": url }));
    }
    for server in &wiring.servers {
        if servers.contains_key(&server.name) {
            continue;
        }
        servers.insert(server.name.clone(), entry(server));
    }
    json!({ "mcpServers": servers })
}

fn entry(server: &McpServerSpec) -> Value {
    match &server.transport {
        McpTransport::Http { url, headers } => {
            let mut e = json!({ "type": "http", "url": url });
            if !headers.is_empty() {
                e["headers"] = pairs(headers);
            }
            e
        }
        McpTransport::Stdio { command, args, env } => {
            let mut e = json!({ "type": "stdio", "command": command, "args": args });
            if !env.is_empty() {
                e["env"] = pairs(env);
            }
            e
        }
    }
}

fn pairs(kv: &[(String, String)]) -> Value {
    Value::Object(kv.iter().map(|(k, v)| (k.clone(), json!(v))).collect())
}

/// Write the config for one run and hand back its path.
///
/// One file per run rather than a shared one: which servers a run gets
/// depends on its agent, so two concurrent runs legitimately differ.
///
/// Synchronous because `Engine::start` is, and this is a sub-kilobyte write
/// happening once as a process is spawned — not worth making the whole trait
/// async over.
pub fn write(dir: &Path, run_key: &str, wiring: &McpWiring) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{run_key}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&config(wiring))?)?;
    Ok(path)
}

/// Where aichip keeps generated MCP configs.
pub fn config_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".aichip")
        .join("mcp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio(name: &str) -> McpServerSpec {
        McpServerSpec {
            name: name.into(),
            transport: McpTransport::Stdio {
                command: "npx".into(),
                args: vec!["-y".into(), "@playwright/mcp".into()],
                env: vec![],
            },
        }
    }

    #[test]
    fn aichips_endpoint_becomes_an_http_server() {
        let c = config(&McpWiring {
            aichip_url: Some("http://127.0.0.1:4820/mcp/run/abc".into()),
            servers: vec![],
        });
        assert_eq!(c["mcpServers"]["aichip"]["type"], "http");
        assert_eq!(c["mcpServers"]["aichip"]["url"], "http://127.0.0.1:4820/mcp/run/abc");
    }

    #[test]
    fn a_user_server_named_aichip_cannot_shadow_the_permission_proxy() {
        // Shadowing it would put a user-controlled process in the path of
        // every approval prompt.
        let c = config(&McpWiring {
            aichip_url: Some("http://real".into()),
            servers: vec![stdio("aichip"), stdio("playwright")],
        });
        assert_eq!(c["mcpServers"]["aichip"]["url"], "http://real");
        assert_eq!(c["mcpServers"]["aichip"]["type"], "http");
        assert_eq!(c["mcpServers"]["playwright"]["command"], "npx");
    }

    #[test]
    fn empty_env_and_headers_are_omitted_rather_than_written_as_empty_objects() {
        let c = config(&McpWiring { aichip_url: None, servers: vec![stdio("p")] });
        assert!(c["mcpServers"]["p"].get("env").is_none());
    }

    #[test]
    fn no_wiring_still_produces_a_valid_empty_config() {
        let c = config(&McpWiring::default());
        assert!(c["mcpServers"].as_object().unwrap().is_empty());
    }
}
