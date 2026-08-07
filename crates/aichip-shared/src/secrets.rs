//! Does this text look like it contains a credential?
//!
//! The companion to [`crate::env_guard`], which answers the same question about
//! a variable *name*. This one reads free text, because aichip now has places a
//! person types prose that is stored and later handed to an agent — a project's
//! Brain, a Skill's instructions — and the honest advice about those is that a
//! secret must never go in one:
//!
//! > Do not store passwords, API keys, tokens, private SSH keys, or recovery
//! > codes in persistent AI context.
//!
//! Saying that in help text is not enough, because the failure is silent: the
//! key sits in a prompt, goes to a model, and is read by anyone who later opens
//! the editor. So the save is refused and the person is told to rotate it,
//! which is the only advice that helps once it has been typed.
//!
//! ## Why this is narrower than `is_auth_env`
//!
//! That check is deliberately broad because a false positive there costs one
//! confusing refusal on a variable nobody needed. Here a false positive blocks
//! somebody from saving a paragraph, and the paragraph they are most likely to
//! write is *about* credentials — "the API key lives in 1Password", "never
//! commit a token". A check that refuses that is a check people route around.
//!
//! So this fires only on the two shapes that are evidence rather than mention:
//! an assignment of a secret-shaped name to a non-trivial value, and a literal
//! that is unmistakably a key.

use crate::env_guard::is_auth_env;

/// What was found, so the refusal can name it without repeating it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The kind of thing, for the message. Never the value itself — echoing a
    /// key into an error that gets logged is the thing being prevented.
    pub what: String,
    /// 1-based line, so the editor can say where.
    pub line: usize,
}

/// Literal shapes that are a credential and cannot be anything else.
///
/// Prefix-matched against a whole word. Each needs a minimum length: `sk-` on
/// its own is a sentence fragment, `sk-ant-api03-…` is a key.
const KEY_SHAPES: &[(&str, usize, &str)] = &[
    ("sk-", 20, "an API key"),
    ("ghp_", 20, "a GitHub token"),
    ("gho_", 20, "a GitHub token"),
    ("ghs_", 20, "a GitHub token"),
    ("github_pat_", 20, "a GitHub token"),
    ("xox", 20, "a Slack token"),
    ("AKIA", 16, "an AWS access key id"),
    ("ASIA", 16, "an AWS access key id"),
    ("AIza", 30, "a Google API key"),
    ("glpat-", 20, "a GitLab token"),
    ("npm_", 30, "an npm token"),
];

/// Values that are obviously not a secret even under a secret-shaped name.
///
/// The one that matters is a placeholder: somebody writing documentation types
/// `API_KEY=your-key-here`, and refusing that teaches them the check is noise.
const PLACEHOLDERS: &[&str] = &[
    "xxx",
    "...",
    "…",
    "your",
    "<",
    "changeme",
    "example",
    "redacted",
    "placeholder",
    "todo",
    "none",
    "null",
    "empty",
];

/// The first thing in `text` that looks like a credential.
///
/// `None` is the answer for ordinary prose, including prose *about* secrets.
pub fn looks_like_secret(text: &str) -> Option<Finding> {
    for (i, line) in text.lines().enumerate() {
        let line_no = i + 1;

        // A PEM block header is unambiguous, and its own line.
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
            return Some(Finding { what: "a private key".into(), line: line_no });
        }

        // `NAME=value` or `NAME: value` where the name reads as a secret.
        if let Some(finding) = assignment(line, line_no) {
            return Some(finding);
        }

        // A literal that can only be a key, wherever it appears.
        for word in line.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',') {
            let word = word.trim_end_matches(|c: char| matches!(c, '.' | ';' | ')'));
            for (prefix, min_len, what) in KEY_SHAPES {
                if word.starts_with(prefix) && word.len() >= *min_len {
                    return Some(Finding { what: (*what).into(), line: line_no });
                }
            }
            // A connection string carrying a password, checked here rather
            // than in `assignment` because the name in front of it is not
            // secret-shaped — `DATABASE_URL` reads as a location, and a bare
            // `postgres://…` pasted into a note has no name at all.
            if word.contains("://") && credentials_in_url(&word.to_ascii_lowercase()) {
                return Some(Finding { what: "a password inside a URL".into(), line: line_no });
            }
        }
    }
    None
}

/// `NAME=value` / `NAME: value` with a secret-shaped name and a real value.
fn assignment(line: &str, line_no: usize) -> Option<Finding> {
    let (name, value) = line
        .split_once('=')
        .or_else(|| line.split_once(':'))?;

    // The name is the last word before the separator, so `export FOO=…` and
    // `  api_key: …` both work.
    let name = name.split_whitespace().last()?.trim_matches('"').trim_matches('\'');
    if name.is_empty() || !is_auth_env(name) {
        return None;
    }

    let value = value.trim().trim_matches('"').trim_matches('\'');
    // A short value is a placeholder or an empty setting, not a leak.
    if value.len() < 8 {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if PLACEHOLDERS.iter().any(|p| lower.starts_with(p)) {
        return None;
    }
    // A URL is a location, not a credential — `API_URL=https://api.example.com`
    // is ordinary. One carrying inline credentials is caught by the
    // `//user:pass@` scan in the caller, which sees every line whether or not
    // the name in front reads as a secret.
    if lower.contains("://") {
        return None;
    }

    Some(Finding {
        what: format!("a value assigned to {name}"),
        line: line_no,
    })
}

/// `scheme://user:password@host` — the one URL that is a credential.
fn credentials_in_url(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("://") else { return false };
    let Some((authority, _)) = rest.split_once('/').or(Some((rest, ""))) else { return false };
    let Some((userinfo, _)) = authority.split_once('@') else { return false };
    // A password, not just a username, and not a placeholder one.
    match userinfo.split_once(':') {
        Some((_, pass)) => pass.len() >= 6 && !PLACEHOLDERS.iter().any(|p| pass.starts_with(p)),
        None => false,
    }
}

/// What to tell somebody who just tried to save one.
///
/// Names where it is and what kind, never the value, and ends where the advice
/// actually ends: a secret that has been typed into a box has to be rotated,
/// whether or not the save went through.
pub fn refusal(f: &Finding) -> String {
    format!(
        "line {} looks like {} — aichip will not store credentials in text it hands to an \
         agent, because that text goes into a prompt and stays readable to anyone who opens \
         this later. Take it out, and rotate it: it has been typed, so treat it as exposed.",
        f.line, f.what
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Option<String> {
        looks_like_secret(text).map(|f| f.what)
    }

    #[test]
    fn the_shapes_that_are_evidence_are_caught() {
        assert_eq!(
            found("ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnop"),
            Some("a value assigned to ANTHROPIC_API_KEY".into())
        );
        assert_eq!(found("token: ghp_abcdefghijklmnopqrstuvwxyz"), Some("a GitHub token".into()));
        assert_eq!(found("use AKIAIOSFODNN7EXAMPLE for s3"), Some("an AWS access key id".into()));
        assert_eq!(
            found("-----BEGIN OPENSSH PRIVATE KEY-----"),
            Some("a private key".into())
        );
        assert_eq!(
            found("postgres://admin:hunter2please@db.internal/app"),
            Some("a password inside a URL".into())
        );
    }

    #[test]
    fn prose_about_secrets_is_not_a_secret() {
        // The paragraph somebody is most likely to write in a project brain,
        // and the one a careless check would refuse — teaching them the check
        // is noise and to route around it.
        for text in [
            "The API key lives in 1Password, under the Deploys vault.",
            "Never commit a token to this repository.",
            "Set ANTHROPIC_API_KEY in your shell before running the tests.",
            "Rotate the AWS access key every 90 days.",
            "See the runbook for how the OAuth flow works.",
        ] {
            assert_eq!(found(text), None, "refused ordinary prose: {text}");
        }
    }

    #[test]
    fn a_location_is_not_a_credential() {
        // The single most common line in a project note.
        assert_eq!(found("DATABASE_URL=postgres://localhost:5432/app"), None);
        assert_eq!(found("API_URL=https://api.example.com/v1"), None);
        // …until it carries a password.
        assert!(found("DATABASE_URL=postgres://u:s3cretpw@host/db").is_some());
    }

    #[test]
    fn a_placeholder_is_not_a_leak() {
        for text in [
            "API_KEY=your-key-here",
            "GITHUB_TOKEN=xxxxxxxxxxxx",
            "SECRET_KEY=<paste it here>",
            "API_KEY=changeme123",
            "OPENAI_API_KEY=",
        ] {
            assert_eq!(found(text), None, "refused a placeholder: {text}");
        }
    }

    #[test]
    fn a_short_prefix_on_its_own_is_a_word() {
        // `sk-` and `xox` appear in ordinary text; only a full-length one is a
        // key. Without the minimum length this refuses the word "skills".
        assert_eq!(found("sk- is a prefix"), None);
        assert_eq!(found("write the sk-eleton first"), None);
        assert_eq!(found("Skills are reusable instructions."), None);
    }

    #[test]
    fn the_refusal_says_where_and_says_to_rotate_it() {
        let text = "How we deploy:\n\nGITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz\n";
        let f = looks_like_secret(text).unwrap();
        assert_eq!(f.line, 3, "the line number is what makes it findable");
        let msg = refusal(&f);
        assert!(msg.contains("line 3"), "{msg}");
        assert!(msg.contains("rotate"), "{msg}");
        // Never echoed back: this string gets logged and shown.
        assert!(!msg.contains("ghp_abcdefghijklmnopqrstuvwxyz"), "{msg}");
    }
}
