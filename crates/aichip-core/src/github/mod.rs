//! GitHub, through the `gh` CLI the user already has.
//!
//! The relationship is the same one aichip has with every other tool it drives:
//! spawn the official binary found on `PATH`, read its stdout, and never touch
//! its configuration. aichip does not read `~/.config/gh`, does not set
//! `GH_TOKEN` or `GITHUB_TOKEN` on anything it spawns, and does not talk to
//! api.github.com itself. That is the four invariants in README.md applied
//! unchanged to a fourth CLI — and it is why this integration needs no
//! credential storage of any kind.
//!
//! Login is checked by *running* `gh auth status`, never by looking for a token
//! file. Same rule as `Engine::detect`, same reason.

pub mod connect;

use serde::Deserialize;
use tokio::process::Command;

/// The binary. Not configurable yet; it is on `PATH` or the feature is off.
const GH: &str = "gh";

/// What one authenticated (or broken) account looks like.
#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub host: String,
    pub login: String,
    pub active: bool,
    pub valid: bool,
    /// Why it cannot be used, in `gh`'s own words — e.g. "HTTP 401: Bad
    /// credentials". Carried verbatim because it is the actionable half of the
    /// message and paraphrasing it would only lose detail. Never a credential:
    /// `gh` reports the *source* of a token, and this never asks for its value.
    pub problem: Option<String>,
}

/// `gh` as this machine has it.
#[derive(Debug, Clone, PartialEq)]
pub struct GitHubInfo {
    pub version: String,
    pub accounts: Vec<Account>,
}

impl GitHubInfo {
    /// Installed *and* logged in somewhere that works.
    ///
    /// The distinction earns its keep: an expired token leaves `gh` installed
    /// and every command failing, and "not installed" and "logged out" want
    /// completely different advice.
    pub fn usable(&self) -> bool {
        self.accounts.iter().any(|a| a.valid)
    }

    /// The account commands will actually run as.
    pub fn active(&self) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|a| a.active)
            .or_else(|| self.accounts.first())
    }
}

/// Is `gh` here, and is it logged in? `None` means not installed.
pub async fn detect() -> Option<GitHubInfo> {
    let version = version().await?;
    Some(GitHubInfo {
        version,
        accounts: accounts().await,
    })
}

async fn version() -> Option<String> {
    let out = Command::new(GH).arg("--version").output().await.ok()?;
    // `gh --version` prints the version and then a release URL; only the first
    // line is the answer.
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    })
}

/// Shapes from `gh auth status --json hosts`.
#[derive(Deserialize)]
struct StatusJson {
    hosts: std::collections::HashMap<String, Vec<HostEntry>>,
}

#[derive(Deserialize)]
struct HostEntry {
    /// "success" when the token works; otherwise "error" (or anything else new,
    /// which is treated as not working rather than assumed fine).
    #[serde(default)]
    state: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    login: String,
}

async fn accounts() -> Vec<Account> {
    // The `--json` form is used rather than the human output on purpose: the
    // plain form writes prose to *stderr* and exits 1 when a token is bad,
    // which is indistinguishable from a dozen other failures. This form exits 0
    // and says which account is broken and why.
    let Ok(out) = Command::new(GH)
        .args(["auth", "status", "--json", "hosts"])
        .output()
        .await
    else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    parse_accounts(&String::from_utf8_lossy(&out.stdout))
}

/// Pure, so the interesting shapes are testable without a GitHub login.
fn parse_accounts(json: &str) -> Vec<Account> {
    let Ok(parsed) = serde_json::from_str::<StatusJson>(json) else {
        return vec![];
    };
    let mut accounts: Vec<Account> = parsed
        .hosts
        .into_iter()
        .flat_map(|(host, entries)| {
            entries.into_iter().map(move |e| Account {
                host: host.clone(),
                login: e.login,
                active: e.active,
                // `gh` says "success", not "valid". Guessing that word wrong
                // is what made a perfectly good login report as unusable, and
                // it stayed hidden because the only account here to test
                // against was genuinely broken — the healthy path was never
                // once exercised. The fixture below is real output.
                valid: e.state == "success",
                problem: (e.state != "success").then(|| {
                    // gh's own words when it has them; otherwise name the state
                    // it reported, so an unfamiliar one is diagnosable rather
                    // than a shrug.
                    e.error.clone().unwrap_or_else(|| {
                        format!("gh reports this login as \"{}\"", e.state)
                    })
                }),
            })
        })
        .collect();
    // Stable order: a HashMap's iteration order would otherwise make `doctor`
    // print a different thing each run on a machine with two hosts.
    accounts.sort_by(|a, b| (&a.host, &a.login).cmp(&(&b.host, &b.login)));
    accounts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real output from gh 2.96.0 on 2026-07-31, with one working account and
    /// one expired one on the same host.
    ///
    /// This fixture exists because every test here used to describe a *failed*
    /// login, so the word `gh` uses for a healthy one was never checked — and
    /// it is "success", not "valid". A perfectly good sign-in reported as
    /// unusable, and no test noticed.
    const MIXED: &str = r#"{"hosts":{"github.com":[
        {"state":"success","active":true,"host":"github.com","login":"koneb71",
         "tokenSource":"keyring","scopes":"gist, read:org, repo","gitProtocol":"ssh"},
        {"state":"error","error":"HTTP 401: Bad credentials (https://api.github.com/)",
         "active":false,"host":"github.com","login":"vaconeiell",
         "tokenSource":"default","gitProtocol":"ssh"}]}}"#;

    #[test]
    fn a_working_login_is_recognised_as_working() {
        let accounts = parse_accounts(MIXED);
        let good = accounts.iter().find(|a| a.login == "koneb71").unwrap();
        assert!(good.valid, "gh said state=success; this must be usable");
        assert!(good.active);
        assert_eq!(good.problem, None);

        // ...and the broken one alongside it is still reported as broken,
        // with gh's own words rather than ours.
        let bad = accounts.iter().find(|a| a.login == "vaconeiell").unwrap();
        assert!(!bad.valid);
        assert!(bad.problem.as_deref().unwrap().contains("401"));
    }

    #[test]
    fn one_working_account_makes_the_install_usable() {
        // The question the whole card hangs on: a broken account sitting
        // beside a good one must not make GitHub unavailable.
        let info = GitHubInfo {
            version: "gh version 2.96.0".into(),
            accounts: parse_accounts(MIXED),
        };
        assert!(info.usable());
        assert_eq!(info.active().unwrap().login, "koneb71");
    }

    #[test]
    fn an_unfamiliar_state_says_which_one_rather_than_shrugging() {
        let odd = r#"{"hosts":{"github.com":[{"state":"pending","active":true,
            "host":"github.com","login":"someone","gitProtocol":"ssh"}]}}"#;
        let a = &parse_accounts(odd)[0];
        assert!(!a.valid);
        assert!(a.problem.as_deref().unwrap().contains("pending"));
    }

    /// The exact shape this machine produced with an expired token.
    const EXPIRED: &str = r#"{"hosts":{"github.com":[{
        "state":"error",
        "error":"HTTP 401: Bad credentials (https://api.github.com/)",
        "active":true,"host":"github.com","login":"vaconeiell",
        "tokenSource":"keyring","gitProtocol":"https"}]}}"#;

    const WORKING: &str = r#"{"hosts":{"github.com":[{
        "state":"success","active":true,"host":"github.com","login":"someone",
        "tokenSource":"keyring","gitProtocol":"ssh","scopes":"repo, read:org"}]}}"#;

    #[test]
    fn an_expired_token_is_installed_but_not_usable() {
        let info = GitHubInfo { version: "gh version 2.96.0".into(), accounts: parse_accounts(EXPIRED) };
        assert!(!info.usable());
        let a = info.active().unwrap();
        assert_eq!(a.login, "vaconeiell");
        assert!(a.active);
        // The reason has to survive to the user; "not logged in" alone would
        // send them to `gh auth login` without saying why they were logged out.
        assert_eq!(a.problem.as_deref(), Some("HTTP 401: Bad credentials (https://api.github.com/)"));
    }

    #[test]
    fn a_working_login_is_usable_and_has_nothing_to_report() {
        let info = GitHubInfo { version: "gh version 2.96.0".into(), accounts: parse_accounts(WORKING) };
        assert!(info.usable());
        assert_eq!(info.active().unwrap().problem, None);
    }

    #[test]
    fn an_unknown_state_is_not_treated_as_working() {
        // A future `gh` inventing a third state must fail closed: offering
        // GitHub actions that then fail is worse than not offering them.
        let odd = WORKING.replace(r#""state":"success""#, r#""state":"expired_soon""#);
        assert!(!parse_accounts(&odd)[0].valid);
    }

    #[test]
    fn no_accounts_at_all_is_logged_out_not_a_crash() {
        assert!(parse_accounts(r#"{"hosts":{}}"#).is_empty());
        assert!(parse_accounts("not json").is_empty());
    }

    #[test]
    fn several_hosts_come_out_in_a_stable_order() {
        let two = r#"{"hosts":{
            "github.com":[{"state":"success","active":true,"login":"b"}],
            "ghe.example":[{"state":"success","active":false,"login":"a"}]}}"#;
        let got: Vec<_> = parse_accounts(two).iter().map(|a| a.host.clone()).collect();
        assert_eq!(got, vec!["ghe.example", "github.com"]);
    }
}
