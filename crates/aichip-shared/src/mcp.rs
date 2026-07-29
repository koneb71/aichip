//! What MCP wiring a run needs, described without reference to any one
//! engine's config format.
//!
//! `RunSpec` used to carry `mcp_config_path: Option<PathBuf>` — a path to a
//! file already written in *Claude's* `{"mcpServers": …}` dialect. That made
//! the orchestrator responsible for knowing each engine's file format, which
//! is the job the `Engine` trait exists to absorb. Worse, it meant the
//! orchestrator gated MCP on the literal string `"claude-code"`, so any other
//! engine silently received no MCP at all: no permission proxy, no workspace
//! tools, no org messaging, and no sign that anything was missing.
//!
//! Now the spec says *what* the run should be able to reach and each adapter
//! writes its own dialect.

use serde::{Deserialize, Serialize};

/// One MCP server, in the terms every client agrees on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpServerSpec {
    /// Namespace the model sees its tools under. Slugged by the caller.
    pub name: String,
    pub transport: McpTransport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTransport {
    /// A process the engine spawns and speaks to over stdio.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Never auth-shaped — validated on the way in by `env_guard`.
        #[serde(default)]
        env: Vec<(String, String)>,
    },
    /// An HTTP endpoint. aichip's own tools arrive this way.
    Http {
        url: String,
        #[serde(default)]
        headers: Vec<(String, String)>,
    },
}

/// Everything a run should be able to reach over MCP.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct McpWiring {
    /// aichip's own endpoint for this run — the permission proxy for board
    /// tasks, workspace tools for chat, team tools for an org member.
    /// `None` disables it (mock engine, utility runs, tests).
    pub aichip_url: Option<String>,
    /// Servers the run's agent opted into.
    pub servers: Vec<McpServerSpec>,
}

impl McpWiring {
    /// Is there anything to wire at all? Adapters skip config generation
    /// entirely when there isn't.
    pub fn is_empty(&self) -> bool {
        self.aichip_url.is_none() && self.servers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_wired_is_recognisably_empty() {
        assert!(McpWiring::default().is_empty());
    }

    #[test]
    fn aichips_own_endpoint_counts_as_wiring() {
        let w = McpWiring {
            aichip_url: Some("http://127.0.0.1:4820/mcp/run/x".into()),
            servers: vec![],
        };
        assert!(!w.is_empty());
    }

    #[test]
    fn a_user_server_alone_counts_too() {
        let w = McpWiring {
            aichip_url: None,
            servers: vec![McpServerSpec {
                name: "playwright".into(),
                transport: McpTransport::Stdio {
                    command: "npx".into(),
                    args: vec!["-y".into(), "@playwright/mcp".into()],
                    env: vec![],
                },
            }],
        };
        assert!(!w.is_empty());
    }
}
