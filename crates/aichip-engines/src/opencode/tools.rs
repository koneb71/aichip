//! Translating aichip's tool names into OpenCode's permission vocabulary.
//!
//! aichip speaks Claude's tool names throughout — `Read`, `Bash`, `MultiEdit`
//! — because that is the CLI it grew up around. OpenCode names the same
//! capabilities differently and, more importantly, its permission config keys
//! off *its* names. A mistranslation here does not fail loudly: it silently
//! grants or denies the wrong capability, which is why this is one small
//! table with exhaustive tests rather than an inline `match`.

/// OpenCode's permission keys, from its published config schema. Anything not
/// in this list is not a key OpenCode understands, and emitting one would make
/// it reject the whole config.
pub const OPENCODE_PERMISSION_KEYS: &[&str] = &[
    "read",
    "edit",
    "glob",
    "grep",
    "list",
    "bash",
    "task",
    "external_directory",
    "lsp",
    "skill",
    "todowrite",
    "question",
    "webfetch",
    "websearch",
    "doom_loop",
];

/// aichip/Claude tool name → OpenCode permission key.
///
/// `None` means "no equivalent" — the caller must not invent one. MCP tools
/// (`mcp__server__tool`) deliberately return `None`: OpenCode namespaces them
/// separately and they are not governed by these keys.
pub fn permission_key(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "Read" | "NotebookRead" => "read",
        "Grep" => "grep",
        "Glob" => "glob",
        "LS" | "List" => "list",
        // OpenCode has one key for all mutation of files.
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => "edit",
        "Bash" | "BashOutput" | "KillShell" => "bash",
        "Task" | "Agent" => "task",
        "TodoWrite" | "TodoRead" => "todowrite",
        "WebFetch" => "webfetch",
        "WebSearch" => "websearch",
        _ => return None,
    })
}

/// Keys that only accept a flat action, never a `{pattern: action}` object.
///
/// Nesting under one of these makes OpenCode reject the config outright, so
/// the builder has to know which is which.
pub fn is_flat_only(key: &str) -> bool {
    matches!(
        key,
        "todowrite" | "question" | "webfetch" | "websearch" | "doom_loop" | "lsp" | "skill"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_aichip_actually_grants_has_a_mapping() {
        // These are the names in `WORKER_TOOLS` and the agent allow-lists. If
        // one of them ever maps to None, that capability silently vanishes on
        // an OpenCode run.
        for tool in [
            "Read",
            "Grep",
            "Glob",
            "Edit",
            "Write",
            "MultiEdit",
            "NotebookEdit",
            "Bash",
            "TodoWrite",
        ] {
            assert!(
                permission_key(tool).is_some(),
                "{tool} has no OpenCode equivalent"
            );
        }
    }

    #[test]
    fn every_mapping_targets_a_key_opencode_understands() {
        // A typo here would be rejected by OpenCode as an invalid config,
        // which surfaces as a run that dies at spawn for no obvious reason.
        for tool in [
            "Read",
            "Grep",
            "Glob",
            "Edit",
            "Bash",
            "Task",
            "TodoWrite",
            "WebFetch",
        ] {
            let key = permission_key(tool).unwrap();
            assert!(
                OPENCODE_PERMISSION_KEYS.contains(&key),
                "{tool} maps to {key}, which is not an OpenCode permission key"
            );
        }
    }

    #[test]
    fn all_file_mutation_collapses_to_edit() {
        for tool in ["Edit", "Write", "MultiEdit", "NotebookEdit"] {
            assert_eq!(permission_key(tool), Some("edit"));
        }
    }

    #[test]
    fn mcp_tools_are_not_forced_into_a_permission_key() {
        // OpenCode namespaces MCP tools separately; pretending one of these
        // is a `task` or a `bash` would grant the wrong thing.
        assert_eq!(permission_key("mcp__aichip__approve"), None);
        assert_eq!(permission_key("mcp__playwright__browser_navigate"), None);
    }

    #[test]
    fn an_unknown_tool_maps_to_nothing_rather_than_guessing() {
        assert_eq!(permission_key("SomeFutureTool"), None);
        assert_eq!(permission_key(""), None);
    }

    #[test]
    fn flat_only_keys_are_identified() {
        assert!(is_flat_only("todowrite"));
        assert!(is_flat_only("webfetch"));
        assert!(
            !is_flat_only("bash"),
            "bash takes {{pattern: action}} rules"
        );
        assert!(!is_flat_only("edit"));
        assert!(!is_flat_only("external_directory"));
    }
}
