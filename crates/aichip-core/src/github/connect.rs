//! Sign in to GitHub without aichip ever holding the credential.
//!
//! `gh auth login --web` runs GitHub's device flow: it prints a one-time code,
//! the person enters that code on github.com, and GitHub hands the token
//! **directly to `gh`**, which puts it in the system credential store. aichip
//! spawns the binary and reads the code off its stderr. That is the whole of
//! its involvement.
//!
//! This is why a "Connect GitHub" button is possible at all without breaking
//! the second invariant in README.md. The alternative — a field to paste a
//! personal access token into — would mean aichip receiving, transporting and
//! storing a credential, which it does not do for any provider and will not
//! start doing for this one. `gh auth login --with-token` exists; it is
//! deliberately not used.
//!
//! The one-time code is not a credential. It authorises this specific pending
//! request and expires; it is shown to the person because showing it *is* the
//! flow.
//!
//! ## Organisations
//!
//! `gh` requires `repo`, `read:org` and `gist`, and says so itself — "the
//! minimum set of scopes cannot be removed". So aichip cannot ask for less, and
//! does not pretend to: it asks for *nothing more* than that unless the person
//! ticks something, and it says what it is asking for before they press the
//! button.
//!
//! What actually decides whether a token can reach an organisation is not the
//! scope, it is GitHub's own authorisation page, which lists each organisation
//! separately with its own Grant control. Granting nothing there leaves the
//! connection personal — which is the honest place to point someone who wants
//! that, rather than a switch here that could not deliver it.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use uuid::Uuid;

/// A device flow waiting for the person to finish it in their browser.
pub struct Pending {
    child: Child,
    started: std::time::Instant,
    /// Everything `gh` has said, for explaining a flow that did not finish.
    said: std::sync::Arc<Mutex<String>>,
}

/// GitHub expires a device code after fifteen minutes; a process still polling
/// for one that died is just a process.
const EXPIRES_AFTER: std::time::Duration = std::time::Duration::from_secs(15 * 60);

fn registry() -> &'static Mutex<HashMap<Uuid, Pending>> {
    static R: OnceLock<Mutex<HashMap<Uuid, Pending>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// What the person needs in order to finish signing in.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    pub id: Uuid,
    /// The one-time code to type into GitHub.
    pub code: String,
    pub url: String,
}

/// How a flow is going.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Progress {
    /// Still waiting for the person to enter the code.
    Waiting,
    Connected,
    Failed { reason: String },
}

/// The one-time code and URL out of `gh`'s own output.
///
/// Pure, because the shape of that output is the only thing this depends on and
/// it is worth pinning: `gh` prints it to *stderr*, not stdout, and phrases it
/// as prose around the code.
pub fn parse_prompt(text: &str) -> Option<(String, String)> {
    let code = text
        .split("one-time code:")
        .nth(1)?
        .split_whitespace()
        .next()?
        .trim()
        .to_string();
    let url = text
        .split_whitespace()
        .find(|w| w.starts_with("https://"))
        .map(|w| w.trim_end_matches(['.', ',']).to_string())
        .unwrap_or_else(|| "https://github.com/login/device".to_string());
    (!code.is_empty()).then_some((code, url))
}

/// What `gh` will not go below, in its own words.
///
/// Surfaced so the UI can state it before the click rather than leaving someone
/// to discover it on GitHub's authorisation page.
pub const REQUIRED_SCOPES: [&str; 3] = ["repo", "read:org", "gist"];

/// Scopes worth offering, none of which are asked for unless chosen.
///
/// Kept short on purpose. Every entry here is a thing the person will be asked
/// to grant, and a list of plausible-sounding extras invites ticking them all.
pub const OPTIONAL_SCOPES: [(&str, &str); 2] = [
    ("workflow", "Push changes to GitHub Actions workflow files"),
    ("read:project", "Read organisation and user projects"),
];

/// Begin a device flow and read back the code to show.
///
/// `extra` is added to what `gh` already requires. Empty is the normal case and
/// the default: asking for more than is needed is how a connection quietly ends
/// up able to do more than anyone intended.
pub async fn start(extra: &[String]) -> anyhow::Result<Started> {
    // Anything not offered is refused rather than passed through: this string
    // goes to a subprocess that talks to GitHub, and it arrives over HTTP.
    let allowed: Vec<&str> = extra
        .iter()
        .filter(|s| OPTIONAL_SCOPES.iter().any(|(name, _)| name == s))
        .map(String::as_str)
        .collect();

    let mut cmd = Command::new("gh");
    cmd.args(["auth", "login", "--web", "--hostname", "github.com"]);
    if !allowed.is_empty() {
        cmd.args(["--scopes", &allowed.join(",")]);
    }
    let mut child = cmd
        // Chosen so nothing is prompted for on a terminal nobody is watching:
        // this process has no console, so any question it asked would hang
        // forever.
        .args(["--git-protocol", "ssh", "--skip-ssh-key"])
        // `gh` opens a browser by default. aichip does not open things on
        // someone's machine unasked — the URL is shown as a link instead.
        .env("BROWSER", "true")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("gh produced no output to read"))?;

    // Drained for the whole life of the process, in a task that outlives this
    // function.
    //
    // This is not tidiness. Reading the code with a reader that then goes out
    // of scope closes the read end of the pipe, and `gh` writes to stderr again
    // the moment the person authorises — "Authentication complete". Writing to
    // a closed pipe kills it, so the process that was about to receive the
    // token died at exactly the moment it was granted. GitHub said yes and
    // nothing was ever stored.
    let said = std::sync::Arc::new(Mutex::new(String::new()));
    let collecting = said.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut tx = Some(tx);
        while let Ok(Some(line)) = lines.next_line().await {
            let found = {
                let mut buf = collecting.lock().unwrap();
                buf.push_str(&line);
                buf.push('\n');
                parse_prompt(&buf)
            };
            if let Some(pair) = found {
                if let Some(tx) = tx.take() {
                    let _ = tx.send(pair);
                }
            }
        }
    });

    let found = tokio::time::timeout(std::time::Duration::from_secs(20), rx).await;
    let Ok(Ok((code, url))) = found else {
        let _ = child.kill().await;
        let seen = said.lock().unwrap().trim().to_string();
        anyhow::bail!(
            "gh did not offer a sign-in code.{}",
            if seen.is_empty() {
                String::new()
            } else {
                format!(" It said: {seen}")
            }
        );
    };

    let id = Uuid::new_v4();
    registry().lock().unwrap().insert(
        id,
        Pending {
            child,
            started: std::time::Instant::now(),
            said,
        },
    );
    Ok(Started { id, code, url })
}

/// Has the person finished? Checked by asking `gh`, never by reading its files.
pub async fn poll(id: Uuid) -> Progress {
    let exited = {
        let mut reg = registry().lock().unwrap();
        let Some(pending) = reg.get_mut(&id) else {
            return Progress::Failed {
                reason: "that sign-in is no longer running".into(),
            };
        };
        if pending.started.elapsed() > EXPIRES_AFTER {
            let _ = pending.child.start_kill();
            reg.remove(&id);
            return Progress::Failed {
                reason: "the code expired before it was used".into(),
            };
        }
        match pending.child.try_wait() {
            Ok(Some(status)) => Some(status.success()),
            Ok(None) => None,
            Err(e) => {
                reg.remove(&id);
                return Progress::Failed {
                    reason: format!("could not check on gh: {e}"),
                };
            }
        }
    };

    match exited {
        None => Progress::Waiting,
        Some(_exit_ok) => {
            let said = registry()
                .lock()
                .unwrap()
                .remove(&id)
                .map(|p| p.said.lock().unwrap().trim().to_string())
                .unwrap_or_default();

            // Whether `gh` can now speak to GitHub is the whole question, and
            // it is the only thing that answers it. The exit code is
            // deliberately ignored: a `gh` that stored the token and then died
            // on the way out is logged in, and reporting failure would send
            // someone to do again what has already worked.
            if super::detect().await.is_some_and(|i| i.usable()) {
                Progress::Connected
            } else {
                Progress::Failed {
                    reason: if said.is_empty() {
                        "sign-in did not complete. Try again.".into()
                    } else {
                        // gh's own last words beat any sentence written here.
                        said.lines().rev().take(3).collect::<Vec<_>>()
                            .into_iter().rev().collect::<Vec<_>>().join(" ")
                    },
                }
            }
        }
    }
}

/// Give up on a flow the person walked away from.
pub async fn cancel(id: Uuid) {
    if let Some(mut pending) = registry().lock().unwrap().remove(&id) {
        let _ = pending.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured from gh 2.96.0 on 2026-07-31. Note it is stderr, and note the
    /// leading `!` — this is prose, not a machine format, so it is worth
    /// having a test that fails loudly if the wording moves.
    const REAL: &str = "\n! First copy your one-time code: 67D6-897D\nOpen this URL to continue in your web browser: https://github.com/login/device\n";

    #[test]
    fn reads_the_code_and_url_out_of_ghs_own_prose() {
        let (code, url) = parse_prompt(REAL).unwrap();
        assert_eq!(code, "67D6-897D");
        assert_eq!(url, "https://github.com/login/device");
    }

    #[test]
    fn waits_rather_than_guessing_when_the_code_has_not_arrived() {
        // Partial output is the normal state for the first moments: returning
        // something here would show a truncated code.
        assert_eq!(parse_prompt(""), None);
        assert_eq!(parse_prompt("! First copy your one-time code:"), None);
        assert_eq!(parse_prompt("some unrelated warning\n"), None);
    }

    #[test]
    fn the_offered_scopes_stay_minimal() {
        // The default asks for nothing beyond what gh already requires. If a
        // scope is ever added to this list it should be a deliberate act, and
        // this test is where that shows up.
        assert_eq!(OPTIONAL_SCOPES.len(), 2);
        for (name, _) in OPTIONAL_SCOPES {
            assert!(
                !REQUIRED_SCOPES.contains(&name),
                "{name} is already required; offering it implies it is optional"
            );
            // Nothing that can change an organisation.
            assert!(!name.starts_with("admin:"), "{name} is too much to offer");
            assert!(!name.starts_with("delete"), "{name} is too much to offer");
        }
    }

    #[test]
    fn falls_back_to_the_known_url_rather_than_failing() {
        // If the wording around the URL changes but the code still parses,
        // sending someone to the right page beats showing them nothing.
        let (code, url) = parse_prompt("one-time code: ABCD-1234\n").unwrap();
        assert_eq!(code, "ABCD-1234");
        assert_eq!(url, "https://github.com/login/device");
    }
}
