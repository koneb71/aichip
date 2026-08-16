//! One rule for "is this an auth secret?", shared by everything that can put
//! an environment variable in front of a spawned agent.
//!
//! This exists to keep compliance invariant 3 — *never set authentication
//! environment variables on spawned processes* — true as the number of
//! engines and providers grows. It replaces two hand-maintained copies of an
//! Anthropic-only prefix list (one in the Claude adapter, one in
//! `mcp_servers`), which would have leaked the moment a second provider
//! existed: nothing in `["ANTHROPIC_", "CLAUDE_CODE_OAUTH"]` stops
//! `OPENAI_API_KEY` or `GOOGLE_APPLICATION_CREDENTIALS`.
//!
//! The check is deliberately broad. A false positive costs someone one
//! confusing refusal; a false negative hands a credential to a subprocess,
//! which is the failure this whole project is organised to avoid.

/// Secrets **aichip itself** puts in its own environment.
///
/// A different problem from the one below, and easy to miss: the guard here
/// only ever ran over variables an adapter was asked to *set*. A spawned CLI
/// inherits the server's whole environment, so the moment aichip gained a
/// credential of its own — object storage, for the knowledge base — every
/// agent it launched would have been handed it, having passed no check at all.
///
/// These are stripped from every child. Deliberately narrow: it names only
/// what aichip owns. Stripping the *user's* variables would be overreach and
/// would break real setups, since OpenCode authenticates some providers from
/// the environment on purpose.
pub const AICHIP_OWN_SECRETS: &[&str] = &["AICHIP_S3_ACCESS_KEY", "AICHIP_S3_SECRET_KEY"];

/// Fragments that make a name look like a secret regardless of vendor.
const SECRET_SUBSTRINGS: &[&str] = &[
    "API_KEY",
    "APIKEY",
    "_TOKEN",
    "TOKEN_",
    "_SECRET",
    "SECRET_",
    "PASSWORD",
    "PASSWD",
    "OAUTH",
    "CREDENTIAL",
    "PRIVATE_KEY",
    "SESSION_KEY",
    "ACCESS_KEY",
];

/// Vendor namespaces we refuse wholesale.
///
/// `OPENCODE_` earns its place twice over: besides credentials it carries
/// `OPENCODE_CONFIG*` and `OPENCODE_PERMISSION`, which can rewrite the very
/// permission rules an adapter generates. The adapter sets its own config
/// variable deliberately, *after* this check has run over user-supplied
/// values.
const VENDOR_PREFIXES: &[&str] = &[
    "ANTHROPIC_",
    "CLAUDE_CODE_OAUTH",
    "OPENAI_",
    "AZURE_",
    "AWS_",
    "GOOGLE_",
    "GEMINI_",
    "VERTEX_",
    "GROQ_",
    "MISTRAL_",
    "DEEPSEEK_",
    "XAI_",
    "OPENROUTER_",
    "TOGETHER_",
    "FIREWORKS_",
    "CEREBRAS_",
    "PERPLEXITY_",
    "COHERE_",
    "HUGGING",
    "HF_",
    "REPLICATE_",
    "OLLAMA_",
    "OPENCODE_",
];

/// Would setting this variable hand an agent a credential — or let it rewrite
/// the rules we generated for it?
pub fn is_auth_env(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    VENDOR_PREFIXES.iter().any(|p| upper.starts_with(p))
        || SECRET_SUBSTRINGS.iter().any(|s| upper.contains(s))
}

/// The refusal message, phrased so the reader understands it is a design
/// stance rather than a bug.
pub fn auth_env_refusal(key: &str) -> String {
    format!(
        "refusing to set {key}: aichip runs on your CLI's own login and never \
         handles credentials, so it will not put auth-shaped environment \
         variables in front of a spawned agent"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_original_anthropic_names_are_still_caught() {
        assert!(is_auth_env("ANTHROPIC_API_KEY"));
        assert!(is_auth_env("CLAUDE_CODE_OAUTH_TOKEN"));
    }

    #[test]
    fn a_second_provider_no_longer_walks_straight_through() {
        // The whole reason this module exists: the old prefix list was
        // Anthropic-only, so every one of these was permitted.
        for key in [
            "OPENAI_API_KEY",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "OPENROUTER_API_KEY",
            "GEMINI_API_KEY",
            "AWS_SECRET_ACCESS_KEY",
            "XAI_API_KEY",
            "OLLAMA_HOST",
        ] {
            assert!(is_auth_env(key), "{key} should be refused");
        }
    }

    #[test]
    fn opencode_config_vars_are_refused_because_they_rewrite_our_rules() {
        // Not secrets, but they can override the generated permission config.
        assert!(is_auth_env("OPENCODE_CONFIG_CONTENT"));
        assert!(is_auth_env("OPENCODE_PERMISSION"));
        assert!(is_auth_env("OPENCODE_API_KEY"));
    }

    #[test]
    fn unknown_vendors_are_caught_by_shape() {
        // A provider nobody has added to the list yet still gets stopped when
        // its variable is named like a secret.
        assert!(is_auth_env("ACME_API_KEY"));
        assert!(is_auth_env("some_service_token"));
        assert!(is_auth_env("DB_PASSWORD"));
        assert!(is_auth_env("MY_OAUTH_THING"));
    }

    #[test]
    fn ordinary_variables_still_pass() {
        for key in [
            "AICHIP_RUN_ID",
            "AICHIP_STEP",
            "MCP_TOOL_TIMEOUT",
            "MCP_TIMEOUT",
            "PWD",
            "PATH",
            "NODE_ENV",
            "DATABASE_URL",
        ] {
            assert!(!is_auth_env(key), "{key} should be allowed");
        }
    }

    #[test]
    fn the_check_ignores_case() {
        assert!(is_auth_env("anthropic_api_key"));
        assert!(is_auth_env("OpenAI_Api_Key"));
    }
}

#[cfg(test)]
mod own_secret_tests {
    use super::*;

    /// Whatever aichip decides to own, it must also recognise as a secret —
    /// otherwise a future addition could be passed through `extra_env` and
    /// sail past the very check that exists to stop it.
    #[test]
    fn everything_aichip_owns_reads_as_a_secret() {
        for key in AICHIP_OWN_SECRETS {
            assert!(is_auth_env(key), "{key} is not recognised as a secret");
        }
    }

    /// The user's own provider credentials are theirs. OpenCode authenticates
    /// some providers from the environment by design, so stripping these would
    /// break working installs to solve a problem aichip didn't create.
    #[test]
    fn the_users_own_credentials_are_left_alone() {
        for key in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "AWS_SECRET_ACCESS_KEY",
        ] {
            assert!(
                !AICHIP_OWN_SECRETS.contains(&key),
                "{key} belongs to the user, not to aichip"
            );
        }
    }
}
