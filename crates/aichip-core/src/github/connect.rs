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

/// Begin a device flow and read back the code to show.
pub async fn start() -> anyhow::Result<Started> {
    let mut child = Command::new("gh")
        .args(["auth", "login", "--web", "--hostname", "github.com"])
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

    // Read until the code appears rather than to EOF: the process stays alive
    // for the whole flow, so waiting for it to finish would wait forever.
    let mut reader = BufReader::new(stderr).lines();
    let mut seen = String::new();
    let found = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while let Ok(Some(line)) = reader.next_line().await {
            seen.push_str(&line);
            seen.push('\n');
            if let Some(pair) = parse_prompt(&seen) {
                return Some(pair);
            }
        }
        None
    })
    .await;

    let Ok(Some((code, url))) = found else {
        let _ = child.kill().await;
        anyhow::bail!(
            "gh did not offer a sign-in code.{}",
            if seen.trim().is_empty() {
                String::new()
            } else {
                format!(" It said: {}", seen.trim())
            }
        );
    };

    let id = Uuid::new_v4();
    registry().lock().unwrap().insert(
        id,
        Pending {
            child,
            started: std::time::Instant::now(),
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
        Some(ok) => {
            registry().lock().unwrap().remove(&id);
            // `gh` exiting zero is not proof: the authoritative answer is
            // whether it can now speak to GitHub, which is what `detect` asks
            // by running `gh auth status`.
            let usable = super::detect().await.is_some_and(|i| i.usable());
            if ok && usable {
                Progress::Connected
            } else {
                Progress::Failed {
                    reason: "sign-in did not complete. Try again.".into(),
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
    fn falls_back_to_the_known_url_rather_than_failing() {
        // If the wording around the URL changes but the code still parses,
        // sending someone to the right page beats showing them nothing.
        let (code, url) = parse_prompt("one-time code: ABCD-1234\n").unwrap();
        assert_eq!(code, "ABCD-1234");
        assert_eq!(url, "https://github.com/login/device");
    }
}
