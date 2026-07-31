//! Ask an agent for a Dockerfile, then make a person read it.
//!
//! Three of four projects on a typical machine have no Dockerfile, so previews
//! do not work for them at all. An agent can write one — it is a well-worn
//! shape and the evidence is right there in the repo.
//!
//! ## Why this is gated
//!
//! A Dockerfile is not configuration. `RUN` executes arbitrary commands at
//! build time, on this machine, with the network. Building an agent-written one
//! without a person reading it is executing agent-authored code on the host —
//! which is precisely the thing every other agent action in aichip stops to ask
//! about. So a written recipe is a *proposal*: stored, shown in full, and never
//! built until someone approves it. Same shape as an agent's wiki edit, and for
//! a much sharper reason.
//!
//! The agent is given no tools and never runs in the project directory. The
//! evidence is gathered here, by code, and pasted into the prompt — so what it
//! saw is exactly what the person reviewing can see, and a repository cannot
//! talk the agent into doing something on its way past.

use std::path::Path;

/// What a project looks like, as far as building it is concerned.
#[derive(Debug, Clone, PartialEq)]
pub struct Survey {
    /// Paths at the root, so the agent can tell a monorepo from a single app.
    pub entries: Vec<String>,
    /// The files that actually decide how a thing is built, with their text.
    pub key_files: Vec<(String, String)>,
}

/// Files worth reading in full. Ordered: the first few matter most, and the
/// budget runs out from the end.
const KEY_FILES: &[&str] = &[
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "Cargo.toml",
    "go.mod",
    "Gemfile",
    "composer.json",
    "pom.xml",
    "build.gradle",
    "Makefile",
    ".nvmrc",
    ".python-version",
    "compose.yaml",
    "docker-compose.yml",
];

/// How much of any one file to include. A lockfile-sized `package.json` is
/// still mostly noise after this, and the parts that decide a build — scripts,
/// engines, dependencies — are at the top.
const PER_FILE_BYTES: usize = 4_000;

pub async fn survey(dir: &Path) -> anyhow::Result<Survey> {
    let mut entries = Vec::new();
    let mut listing = tokio::fs::read_dir(dir).await?;
    while let Some(e) = listing.next_entry().await? {
        let name = e.file_name().to_string_lossy().to_string();
        // Noise that tells you nothing and can be enormous.
        if matches!(name.as_str(), ".git" | "node_modules" | "target" | "dist" | ".venv") {
            continue;
        }
        let suffix = if e.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            "/"
        } else {
            ""
        };
        entries.push(format!("{name}{suffix}"));
    }
    entries.sort();

    let mut key_files = Vec::new();
    for name in KEY_FILES {
        let path = dir.join(name);
        if let Ok(text) = tokio::fs::read_to_string(&path).await {
            let clipped: String = text.chars().take(PER_FILE_BYTES).collect();
            key_files.push((name.to_string(), clipped));
        }
    }
    Ok(Survey { entries, key_files })
}

/// The prompt. Pure, so what the agent is asked is a thing you can read here
/// rather than reconstruct from logs.
pub fn prompt(survey: &Survey) -> String {
    let mut out = String::from(
        "Write a Dockerfile that builds and serves this project, so a reviewer can \
         open it in a browser and look at it.\n\n\
         Requirements:\n\
         - Output ONLY the Dockerfile, in a single ```dockerfile fenced block. No prose.\n\
         - It must end with a long-running server process, not a build that exits.\n\
         - Include an EXPOSE line for the port that server listens on. This is how \
         the preview is published, so it must be right.\n\
         - Build everything inside the image. There are no mounted volumes and no \
         .env file at run time.\n\
         - Do not fetch anything from a private registry, and do not expect secrets.\n\
         - If the project needs a database or another service to start at all, say so \
         in a comment at the top — a preview runs one container and nothing else.\n\n",
    );
    out.push_str("Files at the project root:\n");
    for e in &survey.entries {
        out.push_str(&format!("  {e}\n"));
    }
    for (name, text) in &survey.key_files {
        out.push_str(&format!("\n--- {name} ---\n{text}\n"));
    }
    out
}

/// Pull the Dockerfile out of the reply.
///
/// Models put it in a fenced block even when told not to add prose, so the
/// fence is what we look for, with the whole reply as a fallback. Returns
/// `None` when there is nothing that looks like a Dockerfile at all — better a
/// clear failure than storing an apology as a build recipe.
pub fn extract(reply: &str) -> Option<String> {
    let body = fenced(reply).unwrap_or_else(|| reply.trim().to_string());
    // Every Dockerfile starts with a FROM, and nothing else does. This is the
    // check that catches "I'm sorry, I need more information".
    let has_from = body
        .lines()
        .any(|l| l.trim_start().to_ascii_uppercase().starts_with("FROM "));
    has_from.then_some(body)
}

fn fenced(reply: &str) -> Option<String> {
    let start = reply.find("```")?;
    let after = &reply[start + 3..];
    // Skip the language tag on the fence line, if any.
    let body_start = after.find('\n')? + 1;
    let body = &after[body_start..];
    let end = body.find("```")?;
    Some(body[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_dockerfile_out_of_a_fenced_reply() {
        let reply = "Here you go:\n\n```dockerfile\nFROM node:20\nEXPOSE 3000\n```\n\nHope that helps!";
        assert_eq!(extract(reply).unwrap(), "FROM node:20\nEXPOSE 3000");
    }

    #[test]
    fn accepts_a_bare_dockerfile_with_no_fence() {
        assert_eq!(
            extract("FROM nginx\nEXPOSE 80").unwrap(),
            "FROM nginx\nEXPOSE 80"
        );
        // An unlabelled fence is still a fence.
        assert_eq!(extract("```\nFROM nginx\n```").unwrap(), "FROM nginx");
    }

    #[test]
    fn refuses_a_reply_that_is_not_a_dockerfile() {
        // The realistic failure: the model asks a question instead. Storing
        // that as a recipe would fail at build time with a baffling error.
        assert_eq!(extract("I need to know which port you want."), None);
        assert_eq!(extract(""), None);
        assert_eq!(extract("```\njust some text\n```"), None);
        // A fence containing prose about Dockerfiles is not one either.
        assert_eq!(extract("```\n# This project has no FROM instruction\n```"), None);
    }

    #[test]
    fn the_prompt_carries_the_evidence_and_the_constraints() {
        let s = Survey {
            entries: vec!["package.json".into(), "src/".into()],
            key_files: vec![("package.json".into(), "{\"name\":\"app\"}".into())],
        };
        let p = prompt(&s);
        // The listing and the file contents both have to be in there, or the
        // agent is guessing.
        assert!(p.contains("package.json"));
        assert!(p.contains("{\"name\":\"app\"}"));
        assert!(p.contains("src/"));
        // The constraints that make the result usable as a preview.
        assert!(p.contains("EXPOSE"));
        assert!(p.contains("long-running"));
        // No volumes and no secrets are the two that produce a container which
        // starts and then immediately dies if they are forgotten.
        assert!(p.contains("no mounted volumes"));
        assert!(p.contains("secrets"));
    }
}
