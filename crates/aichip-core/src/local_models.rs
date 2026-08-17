//! What local model runtimes on this machine have available.
//!
//! # This is not an engine, and the distinction is the whole point
//!
//! Ollama and LM Studio are **inference servers**: they serve a model over an
//! HTTP API. They hold no tools, edit no files and run no commands, so a board
//! card handed to one would produce prose *about* editing a file and no edited
//! file. They cannot be `Engine` implementations, and the first compliance
//! invariant says so in as many words — adapters spawn official **agent**
//! binaries on `PATH` and read their **stdout**.
//!
//! What they *are* is providers that OpenCode already fronts. OpenCode is the
//! agent; `ollama/qwen2.5-coder` is a model id it accepts, and
//! [`aichip_shared::model_tier::is_provider_model_shape`] has always validated that
//! shape. Running a local model through aichip works today and needed no code.
//!
//! What did not work was *finding out what you have*. You had to know the
//! exact tag and type it, and a typo surfaced as a run that failed minutes
//! later. This module asks the two runtimes what they have pulled, so the
//! picker can offer it.
//!
//! # Why this does not weaken the invariant
//!
//! It is a read-only `GET` to a loopback port the user is already running, and
//! what comes back is a list of names. No prompt is sent, no completion is
//! requested, nothing is spawned, and no engine traffic passes through
//! aichip — the fourth invariant is about proxying an engine's conversation,
//! and there is no conversation here. If that ever stops being true, this
//! module is the wrong place for it.
//!
//! Nothing fails when neither is running, which is the common case: a refused
//! connection is an empty list, not an error, and never a slow page — hence
//! the short timeout.
//!
//! # Where the addresses come from
//!
//! Settings, with the stock ports as defaults, rather than environment
//! variables. Somebody running Ollama on another port should be able to say so
//! in the dashboard and be found on the next look — an env var means editing a
//! shell profile and restarting the server to change where a *discovery probe*
//! points, which is a lot of ceremony for an optional convenience.

use crate::db::Db;
use serde::Deserialize;
use std::time::Duration;

/// Where each runtime listens when nobody has said otherwise.
///
/// The stock ports, so the common case needs no configuration at all — and
/// they are *defaults* rather than a hardcoding, because a person running
/// Ollama on another port should not have to restart aichip with an
/// environment variable to be found.
pub const OLLAMA_DEFAULT: &str = "http://127.0.0.1:11434";
pub const LMSTUDIO_DEFAULT: &str = "http://127.0.0.1:1234";

const OLLAMA_KEY: &str = "ollama_host";
const LMSTUDIO_KEY: &str = "lmstudio_host";

/// Where to look, as configured.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hosts {
    pub ollama: String,
    pub lmstudio: String,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            ollama: OLLAMA_DEFAULT.to_string(),
            lmstudio: LMSTUDIO_DEFAULT.to_string(),
        }
    }
}

/// What a person may put in the box.
///
/// An address here becomes a request the *server* makes, so the scheme is
/// checked rather than assumed: without it, a value like `file:///etc/passwd`
/// or a bare host would either fail obscurely at request time or reach
/// somewhere nobody meant. aichip is a single-operator tool and this is the
/// operator's own setting, so this is a guard against a typo rather than
/// against an attacker — which is why it refuses with a sentence rather than
/// silently correcting.
pub fn vet_host(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() {
        return Err("give an address, or leave it empty to use the default".into());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("{url} needs to start with http:// or https://"));
    }
    if url.contains(char::is_whitespace) {
        return Err("an address cannot contain spaces".into());
    }
    Ok(url.to_string())
}

/// Read the configured addresses, falling back to the defaults.
///
/// Never fails: a database hiccup gives the defaults, because the alternative
/// is a settings page that cannot render because it cannot read where to look
/// for something optional.
pub async fn hosts(db: &Db) -> Hosts {
    let read = |key: &'static str| async move {
        sqlx::query_scalar::<_, serde_json::Value>("SELECT value FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value::<String>(v).ok())
            .and_then(|v| vet_host(&v).ok())
    };
    let (ollama, lmstudio) = tokio::join!(read(OLLAMA_KEY), read(LMSTUDIO_KEY));
    let d = Hosts::default();
    Hosts {
        ollama: ollama.unwrap_or(d.ollama),
        lmstudio: lmstudio.unwrap_or(d.lmstudio),
    }
}

/// Store one or both. An empty string clears back to the default rather than
/// saving a blank, so "reset this" is a thing the box itself can express.
pub async fn set_hosts(
    db: &Db,
    ollama: Option<&str>,
    lmstudio: Option<&str>,
) -> Result<(), String> {
    for (key, value) in [(OLLAMA_KEY, ollama), (LMSTUDIO_KEY, lmstudio)] {
        let Some(raw) = value else { continue };
        if raw.trim().is_empty() {
            let _ = sqlx::query("DELETE FROM settings WHERE key = $1")
                .bind(key)
                .execute(&db.pool)
                .await;
            continue;
        }
        let url = vet_host(raw)?;
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(serde_json::json!(url))
        .execute(&db.pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A model one of the local runtimes has, named the way OpenCode wants it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    /// `ollama` or `lmstudio` — the provider half of the id.
    pub provider: &'static str,
    /// The full `provider/model` id, ready to paste into a model field.
    pub id: String,
    /// Just the model half, for a picker that shows the provider separately.
    pub name: String,
}

/// Nobody waits for a runtime that is not running.
///
/// Loopback either answers immediately or is not there. Two seconds is long
/// enough for a busy Ollama to reply and short enough that a settings page
/// does not feel broken when neither is installed.
const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct OllamaTags {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct OpenAiModels {
    #[serde(default)]
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    #[serde(default)]
    id: String,
}

/// Ollama's `/api/tags` body into ids. Pure, so the shape is tested without a
/// server.
pub fn parse_ollama(body: &str) -> Vec<LocalModel> {
    let Ok(tags) = serde_json::from_str::<OllamaTags>(body) else {
        return vec![];
    };
    tidy(
        "ollama",
        tags.models.into_iter().map(|m| m.name).collect::<Vec<_>>(),
    )
}

/// LM Studio speaks the OpenAI models shape, so this reads `data[].id`.
pub fn parse_lmstudio(body: &str) -> Vec<LocalModel> {
    let Ok(models) = serde_json::from_str::<OpenAiModels>(body) else {
        return vec![];
    };
    tidy(
        "lmstudio",
        models.data.into_iter().map(|m| m.id).collect::<Vec<_>>(),
    )
}

/// Sorted, de-duplicated, and free of anything that would not survive being a
/// model id.
///
/// Sorted because the picker shows it and a list that reorders between polls
/// reads as though something changed. A name containing whitespace or a slash
/// is dropped rather than escaped: `is_provider_model_shape` would reject it
/// downstream anyway, and offering an id that cannot be saved is worse than
/// omitting it.
fn tidy(provider: &'static str, names: Vec<String>) -> Vec<LocalModel> {
    let mut out: Vec<LocalModel> = names
        .into_iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && !n.contains(char::is_whitespace) && !n.contains('/'))
        .map(|name| LocalModel {
            provider,
            id: format!("{provider}/{name}"),
            name,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

async fn fetch(url: String) -> Option<String> {
    let client = reqwest::Client::builder().timeout(TIMEOUT).build().ok()?;
    let res = client.get(&url).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    res.text().await.ok()
}

/// Everything both runtimes have, or an empty list.
///
/// Both are probed concurrently: they are independent, and doing them in
/// sequence would make the page wait twice for two things that are usually
/// both absent.
pub async fn discover(db: &Db) -> Vec<LocalModel> {
    let Hosts { ollama, lmstudio } = hosts(db).await;
    let (a, b) = tokio::join!(
        fetch(format!("{ollama}/api/tags")),
        fetch(format!("{lmstudio}/v1/models")),
    );
    let mut out = a.as_deref().map(parse_ollama).unwrap_or_default();
    out.extend(b.as_deref().map(parse_lmstudio).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_ollama_reports() {
        // Trimmed from a real /api/tags body.
        let out = parse_ollama(
            r#"{"models":[
                 {"name":"qwen2.5-coder:7b","size":4683087332},
                 {"name":"llama3:latest","size":4661224676}]}"#,
        );
        assert_eq!(out.len(), 2);
        // Sorted, so the picker does not reshuffle between polls.
        assert_eq!(out[0].id, "ollama/llama3:latest");
        assert_eq!(out[1].id, "ollama/qwen2.5-coder:7b");
        assert_eq!(out[1].name, "qwen2.5-coder:7b");
    }

    #[test]
    fn reads_lm_studios_openai_shape() {
        let out =
            parse_lmstudio(r#"{"object":"list","data":[{"id":"qwen2.5-coder-7b-instruct"}]}"#);
        assert_eq!(out[0].id, "lmstudio/qwen2.5-coder-7b-instruct");
        assert_eq!(out[0].provider, "lmstudio");
    }

    #[test]
    fn the_ids_it_offers_are_ones_opencode_would_accept() {
        // The whole point of the feature: what it hands the picker has to
        // survive the validation the save path applies.
        for m in parse_ollama(r#"{"models":[{"name":"qwen2.5-coder:7b"}]}"#) {
            assert!(
                aichip_shared::model_tier::is_provider_model_shape(&m.id),
                "{}",
                m.id
            );
        }
    }

    #[test]
    fn a_name_that_could_not_be_an_id_is_dropped_not_mangled() {
        // A slash would make a three-part id, and whitespace fails the shape
        // check. Offering something unsaveable is worse than omitting it.
        let out = parse_ollama(
            r#"{"models":[{"name":"has space"},{"name":"org/model"},{"name":"  "},{"name":"fine"}]}"#,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "ollama/fine");
    }

    #[test]
    fn an_address_has_to_be_one() {
        // The value becomes a request the server makes, so a typo is caught
        // in the box rather than at probe time.
        assert!(
            vet_host("127.0.0.1:11434").is_err(),
            "a bare host is not a URL"
        );
        assert!(vet_host("file:///etc/passwd").is_err());
        assert!(vet_host("http://host name").is_err());
        assert!(vet_host("").is_err());
        // Trailing slash is tidied rather than refused: people paste it.
        assert_eq!(
            vet_host("http://127.0.0.1:11434/").unwrap(),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            vet_host(" https://box.local:1234 ").unwrap(),
            "https://box.local:1234"
        );
    }

    #[test]
    fn the_defaults_are_the_stock_ports_and_survive_their_own_check() {
        // A default that its own validator rejects would make the common case
        // the broken one.
        let d = Hosts::default();
        assert_eq!(vet_host(&d.ollama).unwrap(), OLLAMA_DEFAULT);
        assert_eq!(vet_host(&d.lmstudio).unwrap(), LMSTUDIO_DEFAULT);
    }

    #[test]
    fn nothing_running_is_an_empty_list_not_an_error() {
        // The common case. A settings page must not show an error because a
        // thing the user never installed is not answering.
        assert!(parse_ollama("").is_empty());
        assert!(parse_ollama("<html>404</html>").is_empty());
        assert!(parse_lmstudio(r#"{"data":[]}"#).is_empty());
        assert!(parse_ollama(r#"{"models":[]}"#).is_empty());
    }
}
