//! MCP servers the user connects themselves.
//!
//! aichip has always written an MCP config for every run — it is how the
//! permission proxy and the org tools reach the model. This module lets that
//! same file carry servers the user chose, which is the difference between
//! an agent that can only edit files and one that can drive a browser or
//! query a real schema.
//!
//! Two rules hold the safety story together, and both live here rather than
//! in a prompt:
//!
//! 1. A server reaches a run only if the run's agent opted into it. No
//!    agent, no extra servers — chat and utility runs stay as they were.
//! 2. Auth-shaped environment variables are refused, the same names the
//!    process spawner refuses. A user-supplied MCP server is not a hole to
//!    smuggle `ANTHROPIC_API_KEY` through.

use crate::db::Db;
use serde_json::{json, Map, Value};
use sqlx::Row;
use uuid::Uuid;

// The rule lives in `aichip_shared::env_guard` so this and both adapters
// enforce exactly one definition of "auth-shaped".

#[derive(Debug, Clone)]
pub struct McpServer {
    pub id: Uuid,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Value,
    pub url: Option<String>,
    pub headers: Value,
}

impl McpServer {
    /// The entry as it goes into the `mcpServers` object of an MCP config.
    pub fn config_entry(&self) -> Value {
        match self.transport.as_str() {
            "http" | "sse" => {
                let mut entry = json!({
                    "type": self.transport,
                    "url": self.url.clone().unwrap_or_default(),
                });
                if let Some(headers) = self.headers.as_object().filter(|h| !h.is_empty()) {
                    entry["headers"] = Value::Object(headers.clone());
                }
                entry
            }
            _ => {
                let mut entry = json!({
                    "type": "stdio",
                    "command": self.command.clone().unwrap_or_default(),
                    "args": self.args,
                });
                if let Some(env) = self.env.as_object().filter(|e| !e.is_empty()) {
                    entry["env"] = Value::Object(env.clone());
                }
                entry
            }
        }
    }

    /// Engine-neutral description, for `RunSpec.mcp`.
    pub fn to_spec(&self) -> aichip_shared::McpServerSpec {
        let pairs = |v: &Value| -> Vec<(String, String)> {
            v.as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, x)| x.as_str().map(|x| (k.clone(), x.to_string())))
                        .collect()
                })
                .unwrap_or_default()
        };
        let transport = match self.transport.as_str() {
            "http" | "sse" => aichip_shared::McpTransport::Http {
                url: self.url.clone().unwrap_or_default(),
                headers: pairs(&self.headers),
            },
            _ => aichip_shared::McpTransport::Stdio {
                command: self.command.clone().unwrap_or_default(),
                args: self.args.clone(),
                env: pairs(&self.env),
            },
        };
        aichip_shared::McpServerSpec { name: self.name.clone(), transport }
    }

    /// What to put in `--allowedTools` to permit this server's tools.
    ///
    /// The server-level prefix rather than `mcp__name__tool` for each tool:
    /// tool names aren't known until the server is running, and enumerating
    /// them would mean connecting at run-start just to build a flag. Task
    /// runs also go through the permission proxy, so anything this doesn't
    /// cover surfaces as a prompt to the user rather than a silent failure.
    pub fn tool_prefix(&self) -> String {
        format!("mcp__{}", self.name)
    }
}

/// Reject the names that would let a config file do what the process spawner
/// refuses to do.
pub fn check_env(env: &Value) -> anyhow::Result<()> {
    let Some(map) = env.as_object() else {
        return Ok(());
    };
    for key in map.keys() {
        if aichip_shared::is_auth_env(key) {
            anyhow::bail!("{}", aichip_shared::auth_env_refusal(key));
        }
    }
    Ok(())
}

/// The servers a given agent may use. `None` agent means none: an unbound
/// run gets exactly the tools it always had.
pub async fn for_agent(db: &Db, agent_id: Option<Uuid>) -> anyhow::Result<Vec<McpServer>> {
    let Some(agent_id) = agent_id else {
        return Ok(vec![]);
    };
    let rows = sqlx::query(
        "SELECT s.id, s.name, s.transport, s.command, s.args, s.env, s.url, s.headers
         FROM mcp_servers s
         JOIN agent_mcp_servers a ON a.server_id = s.id
         WHERE a.agent_id = $1 AND s.enabled
         ORDER BY s.name",
    )
    .bind(agent_id)
    .fetch_all(&db.pool)
    .await?;

    Ok(rows.iter().map(row_to_server).collect())
}

pub async fn list(db: &Db, workspace_id: Uuid) -> anyhow::Result<Vec<McpServer>> {
    let rows = sqlx::query(
        "SELECT id, name, transport, command, args, env, url, headers
         FROM mcp_servers WHERE workspace_id = $1 ORDER BY name",
    )
    .bind(workspace_id)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows.iter().map(row_to_server).collect())
}

fn row_to_server(r: &sqlx::postgres::PgRow) -> McpServer {
    McpServer {
        id: r.get("id"),
        name: r.get("name"),
        transport: r.get("transport"),
        command: r.get("command"),
        args: r.get("args"),
        env: r.get("env"),
        url: r.get("url"),
        headers: r.get("headers"),
    }
}

/// Merge user servers into aichip's own config object.
///
/// aichip's entry wins on a name collision — a user server called "aichip"
/// must not be able to shadow the permission proxy, which is the channel the
/// user approves tool calls through.
pub fn merge_into(config: &mut Value, servers: &[McpServer]) {
    let Some(map) = config
        .get_mut("mcpServers")
        .and_then(|m| m.as_object_mut())
    else {
        return;
    };
    let mut extra = Map::new();
    for server in servers {
        if map.contains_key(&server.name) {
            continue;
        }
        extra.insert(server.name.clone(), server.config_entry());
    }
    map.extend(extra);
}

/// Slug for the `mcp__<name>__<tool>` namespace: letters, digits, and
/// underscores only, because anything else changes how the tool name parses.
pub fn slug_name(raw: &str) -> String {
    let slug: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    slug.chars().take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(name: &str, transport: &str) -> McpServer {
        McpServer {
            id: Uuid::new_v4(),
            name: name.into(),
            transport: transport.into(),
            command: Some("npx".into()),
            args: vec!["-y".into(), "@playwright/mcp".into()],
            env: json!({}),
            url: Some("https://example.test/mcp".into()),
            headers: json!({}),
        }
    }

    #[test]
    fn a_stdio_server_becomes_a_command_entry() {
        let entry = server("playwright", "stdio").config_entry();
        assert_eq!(entry["type"], "stdio");
        assert_eq!(entry["command"], "npx");
        assert_eq!(entry["args"][1], "@playwright/mcp");
        assert!(entry.get("url").is_none());
    }

    #[test]
    fn an_http_server_becomes_a_url_entry() {
        let entry = server("linear", "http").config_entry();
        assert_eq!(entry["type"], "http");
        assert_eq!(entry["url"], "https://example.test/mcp");
        assert!(entry.get("command").is_none());
    }

    #[test]
    fn empty_env_and_headers_are_left_out_entirely() {
        // An explicit `"env": {}` is noise in a config a user may well read.
        let entry = server("plain", "stdio").config_entry();
        assert!(entry.get("env").is_none());
        assert!(server("plain", "http").config_entry().get("headers").is_none());
    }

    #[test]
    fn auth_env_is_refused() {
        for key in ["ANTHROPIC_API_KEY", "anthropic_api_key", "CLAUDE_CODE_OAUTH_TOKEN"] {
            let err = check_env(&json!({ key: "sk-whatever" })).unwrap_err();
            assert!(err.to_string().contains(key), "{key} should be named in the error");
        }
    }

    #[test]
    fn ordinary_env_is_fine() {
        check_env(&json!({ "DATABASE_URL": "postgres://localhost/x", "PORT": "5432" })).unwrap();
    }

    #[test]
    fn merging_never_lets_a_user_server_shadow_aichip() {
        // The aichip entry is the permission proxy. Shadowing it would put a
        // user-controlled process in the path of every approval prompt.
        let mut config = json!({ "mcpServers": { "aichip": { "type": "http", "url": "real" } } });
        merge_into(&mut config, &[server("aichip", "stdio"), server("playwright", "stdio")]);
        assert_eq!(config["mcpServers"]["aichip"]["url"], "real");
        assert_eq!(config["mcpServers"]["playwright"]["type"], "stdio");
    }

    #[test]
    fn the_tool_prefix_matches_the_mcp_naming_scheme() {
        assert_eq!(server("playwright", "stdio").tool_prefix(), "mcp__playwright");
    }

    #[test]
    fn names_are_slugged_into_the_tool_namespace() {
        assert_eq!(slug_name("  Play Wright! "), "play_wright");
        assert_eq!(slug_name("linear-mcp"), "linear_mcp");
        assert_eq!(slug_name("Postgres (prod)"), "postgres_prod");
    }
}
