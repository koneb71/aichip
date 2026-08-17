//! Reading a local runtime's own CLI output.
//!
//! Pure, and separated for the reason the Codex adapter's header regrets not
//! being able to claim: every parser here was written against output captured
//! from the real binary on this machine, and those transcripts are the
//! fixtures below. `ollama` 0.5.7 and `lms` (CLI commit 71bd99c).
//!
//! The general rule, borrowed from `opencode::parse_models`: a line that is
//! not the shape we expect is **dropped, never guessed at**. These tools
//! decorate their output for humans and are free to add a header or a footer
//! in the next release; the cost of a wrong guess is a model id offered in a
//! picker that no runtime will answer to.

use crate::opencode::strip_ansi;

/// Model names out of `ollama list`.
///
/// The output is a plain table:
///
/// ```text
/// NAME                  ID              SIZE      MODIFIED
/// deepseek-r1:latest    0a8c26691023    4.7 GB    18 months ago
/// ```
///
/// Telling a row from a sentence needs more than "has two columns": when the
/// daemon is down, `ollama list` prints *could not connect to a running
/// Ollama instance*, and reading that as a model named `could` would put it
/// in a picker. So a row must also carry one of the table's own structural
/// marks — a hex digest in the second column, or the `name:tag` shape ollama
/// always prints. Prose has neither; a layout change loses models, which is
/// the safe direction, because an engine with no models is not offered at
/// all rather than offered and broken.
///
/// Embedding models are dropped for the
/// same reason `local_models::tidy` drops them — `ollama list` does not say
/// what a model is *for*, and handing `nomic-embed-text` to a coding agent
/// costs a failed run to discover. Ollama's own naming is consistent enough
/// for that to be a narrow heuristic rather than a guess about capability.
pub fn ollama_models(stdout: &str) -> Vec<String> {
    let mut out: Vec<String> = stdout
        .lines()
        .map(strip_ansi)
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let name = cols.next()?;
            // The header. Checked by name rather than by position so a future
            // version printing a blank line first doesn't eat a real row.
            if name.eq_ignore_ascii_case("NAME") {
                return None;
            }
            let digest = cols.next()?;
            // `name:tag`, with both halves present — `Error:` is not a model,
            // and a trailing colon is how every diagnostic these tools print
            // begins.
            let tagged = name
                .split_once(':')
                .is_some_and(|(n, tag)| !n.is_empty() && !tag.is_empty());
            let digested = digest.len() >= 8 && digest.chars().all(|c| c.is_ascii_hexdigit());
            if !(tagged || digested) {
                return None;
            }
            let low = name.to_lowercase();
            if low.contains("embed") || low.contains("rerank") {
                return None;
            }
            Some(name.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LmsEntry {
    /// `llm` or `embedding`. LM Studio *says*, which is why this adapter
    /// needs no name heuristic where the Ollama one does.
    #[serde(default, rename = "type")]
    kind: String,
    /// Exactly the id the server answers to, publisher namespace and all —
    /// `google/gemma-4-e4b`, not the display name.
    #[serde(default)]
    model_key: String,
}

/// Model ids out of `lms ls --json`.
///
/// The JSON form rather than the table on purpose: the table renders
/// `google/gemma-4-e4b (1 variant)` and a `✓ LOADED` marker in the same
/// columns, so parsing it means stripping decoration off the one field that
/// has to be exact.
pub fn lmstudio_models(json: &str) -> Vec<String> {
    let Ok(entries) = serde_json::from_str::<Vec<LmsEntry>>(json) else {
        return vec![];
    };
    let mut out: Vec<String> = entries
        .into_iter()
        .filter(|e| e.kind == "llm")
        .map(|e| e.model_key.trim().to_string())
        .filter(|k| !k.is_empty() && !k.contains(char::is_whitespace))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// What `lms status` says about the local server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerState {
    pub on: bool,
    /// The port it reported, when it did. Authoritative in a way a setting is
    /// not: the port is changed in LM Studio's own UI, and nobody who does
    /// that thinks to come and tell aichip.
    pub port: Option<u16>,
}

/// `Server: ON (port: 1234)` → on, 1234.
///
/// Deliberately not "does something answer on 1234" — that would be the HTTP
/// probe the settings page uses, and an adapter is held to the stricter rule.
pub fn lms_status(stdout: &str) -> ServerState {
    for line in stdout.lines().map(strip_ansi) {
        let Some(rest) = line.trim().strip_prefix("Server:") else {
            continue;
        };
        let rest = rest.trim();
        // Anything that isn't a clear yes is a no. An unrecognised word here
        // must not read as "running", because the consequence of that is an
        // engine offered to somebody whose server is off.
        if !rest.to_ascii_uppercase().starts_with("ON") {
            return ServerState::default();
        }
        let port = rest
            .split_once("port:")
            .map(|(_, p)| p.trim_matches(|c: char| !c.is_ascii_digit()))
            .and_then(|p| p.parse().ok());
        return ServerState { on: true, port };
    }
    ServerState::default()
}

/// `ollama version is 0.5.7` → `0.5.7`.
///
/// Falls back to the whole first line, so a reworded banner shows something
/// truthful rather than nothing.
pub fn ollama_version(stdout: &str) -> Option<String> {
    let line = stdout
        .lines()
        .map(strip_ansi)
        .find(|l| !l.trim().is_empty())?;
    let line = line.trim();
    Some(match line.rsplit_once(" is ") {
        Some((_, v)) if !v.trim().is_empty() => v.trim().to_string(),
        _ => line.to_string(),
    })
}

/// The one useful line out of `lms version`, which is otherwise ASCII art, a
/// sentence about what `lms` is, and three links.
pub fn lms_version(stdout: &str) -> Option<String> {
    stdout.lines().map(strip_ansi).find_map(|l| {
        Some(format!(
            "cli {}",
            l.trim().strip_prefix("CLI commit:")?.trim()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim from `ollama list` on 2026-08-17, trailing spaces and all.
    const OLLAMA_LIST: &str = "NAME                  ID              SIZE      MODIFIED      \n\
                               deepseek-r1:latest    0a8c26691023    4.7 GB    18 months ago    \n";

    #[test]
    fn reads_the_table_ollama_actually_prints() {
        assert_eq!(ollama_models(OLLAMA_LIST), vec!["deepseek-r1:latest"]);
    }

    #[test]
    fn the_header_is_not_a_model_and_neither_is_a_sentence() {
        // Ollama prints the header alone when nothing is pulled, and the
        // engine is not offered at all in that case — so reading it as a
        // model called "NAME" would be the difference between "not offered"
        // and "offered, then fails at spawn".
        assert!(ollama_models("NAME    ID    SIZE    MODIFIED\n").is_empty());
        assert!(ollama_models("").is_empty());
        // What ollama prints when its daemon is down. Two columns and then
        // some, so "has more than one column" was not enough of a test — this
        // used to come back as a model called `could`.
        assert!(ollama_models("could not connect to a running Ollama instance\n").is_empty());
        assert!(ollama_models("Error: something went wrong here\n").is_empty());
    }

    #[test]
    fn an_embedding_model_is_not_offered_as_something_that_can_answer() {
        // `nomic-embed-text` is one of the most commonly pulled models there
        // is, and it cannot hold a conversation.
        let out = ollama_models(
            "NAME                     ID      SIZE   MODIFIED\n\
             nomic-embed-text:latest  aaa     274 MB 2 days ago\n\
             qwen2.5-coder:7b         bbb     4.7 GB 2 days ago\n",
        );
        assert_eq!(out, vec!["qwen2.5-coder:7b"]);
    }

    /// Trimmed from `lms ls --json` on 2026-08-17 — the fields this reads,
    /// with the rest of each object's twenty keys removed.
    const LMS_LS: &str = r#"[
      {"type":"llm","modelKey":"qwen3.5-9b-claude-deckard-agent-coder-heretic-qx86-hi-mlx",
       "displayName":"Qwen3.5 9B","maxContextLength":262144},
      {"type":"llm","modelKey":"gemma-4-e4b-agentic-opus-reasoning-geminicli"},
      {"type":"llm","modelKey":"google/gemma-4-e4b"},
      {"type":"embedding","modelKey":"text-embedding-nomic-embed-text-v1.5"}
    ]"#;

    #[test]
    fn reads_the_ids_lm_studio_serves_not_the_names_it_displays() {
        // `modelKey` is what `/v1/models` answers to; `displayName` is
        // "Qwen3.5 9B", which is not a model id and has spaces in it.
        let out = lmstudio_models(LMS_LS);
        assert_eq!(
            out,
            vec![
                "gemma-4-e4b-agentic-opus-reasoning-geminicli",
                "google/gemma-4-e4b",
                "qwen3.5-9b-claude-deckard-agent-coder-heretic-qx86-hi-mlx",
            ]
        );
    }

    #[test]
    fn lm_studio_says_which_models_are_embeddings_so_no_heuristic_is_needed() {
        // The Ollama path guesses from the name because `ollama list` gives
        // it nothing else. This one is told, and must use that.
        assert!(!lmstudio_models(LMS_LS)
            .iter()
            .any(|m| m.contains("nomic-embed")));
        // A chat model whose name happens to contain the word survives here,
        // which is exactly the case the name heuristic would get wrong.
        let out = lmstudio_models(r#"[{"type":"llm","modelKey":"embedder-chat-7b"}]"#);
        assert_eq!(out, vec!["embedder-chat-7b"]);
    }

    #[test]
    fn a_namespaced_id_is_kept_whole() {
        // `lmstudio/google/gemma-4-e4b` is three segments and entirely
        // ordinary; `is_provider_model_shape` was widened for exactly this.
        let out = lmstudio_models(r#"[{"type":"llm","modelKey":"google/gemma-4-e4b"}]"#);
        assert!(aichip_shared::is_provider_model_shape(&format!(
            "lmstudio/{}",
            out[0]
        )));
    }

    #[test]
    fn nothing_parseable_is_an_empty_list_not_a_panic() {
        assert!(lmstudio_models("").is_empty());
        assert!(lmstudio_models("command not found").is_empty());
        assert!(lmstudio_models("[]").is_empty());
    }

    #[test]
    fn reads_the_port_lm_studio_reports_rather_than_the_one_we_assumed() {
        // The port is changed in LM Studio's own UI, and nobody who does that
        // comes and tells aichip.
        let s = lms_status("Server: ON (port: 1234)\n\nLoaded Models\n  · google/gemma-4-e4b\n");
        assert_eq!(
            s,
            ServerState {
                on: true,
                port: Some(1234)
            }
        );
        assert_eq!(lms_status("Server: ON (port: 8080)").port, Some(8080));
    }

    #[test]
    fn anything_that_is_not_a_clear_yes_is_a_no() {
        // Getting this backwards offers the engine to somebody whose server
        // is off, and every run they start dies at spawn.
        assert!(!lms_status("Server: OFF").on);
        assert!(!lms_status("").on);
        assert!(!lms_status("Error: LM Studio is not running").on);
        assert!(!lms_status("Server: stopping").on);
    }

    #[test]
    fn versions_come_out_of_banners_written_for_people() {
        assert_eq!(
            ollama_version("ollama version is 0.5.7\n").unwrap(),
            "0.5.7"
        );
        // The real `lms version` output is four lines of ASCII art, a
        // sentence, the commit, and three links.
        let banner = "   __   __  ___\n\nlms is LM Studio's CLI utility for your models.\n\
                      CLI commit: 71bd99c\n\nDocs: https://lmstudio.ai/docs/developer\n";
        assert_eq!(lms_version(banner).unwrap(), "cli 71bd99c");
        assert!(lms_version("no version here").is_none());
    }

    #[test]
    fn a_reworded_version_banner_still_says_something_true() {
        assert_eq!(ollama_version("ollama 9.9.9\n").unwrap(), "ollama 9.9.9");
        assert!(ollama_version("   \n\n").is_none());
    }
}
