//! What to build, and which port to publish — decided from the branch itself.
//!
//! Pure: given a directory listing and a Dockerfile's text, say what to do.
//! Every decision here is one a person could check by reading the same file,
//! which is the point — a preview that runs something you did not ask for is
//! worse than no preview.
//!
//! Deliberately narrow. Slice 1 handles exactly one shape: a `Dockerfile` at
//! the root of the branch. Compose is handled by `compose.rs` instead, and
//! `start` prefers it when a branch has both — this module's narrowness was
//! deliberate rather than an omission, because compose brings multi-service graphs, named
//! volumes and user-defined networks, and those are what turn "preview one
//! card" into "quietly exhaust the machine your other projects run on".

/// Where a preview's container port comes from, so the UI can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortSource {
    /// An `EXPOSE` line in the Dockerfile said so.
    Exposed,
    /// Nothing said. We guessed, and the UI admits it.
    Assumed,
}

/// The container port a preview should publish, and how we know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Port {
    pub number: u16,
    pub source: PortSource,
}

/// What a branch with no `EXPOSE` gets. Wrong as often as right, which is why
/// `PortSource::Assumed` travels with it instead of being smoothed over.
pub const ASSUMED_PORT: u16 = 80;

/// Why this branch cannot be previewed, phrased for the person reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoRecipe {
    /// No Dockerfile at the root of the branch.
    NoDockerfile,
}

impl NoRecipe {
    pub fn message(&self) -> &'static str {
        match self {
            // Names the file and the place, because "cannot preview" sends
            // people looking in settings for a switch that does not exist.
            Self::NoDockerfile => {
                "This branch has no Dockerfile at its root, so there is nothing to build. \
                 Add one and try again."
            }
        }
    }
}

/// The first port a Dockerfile exposes.
///
/// `EXPOSE` takes several shapes — bare numbers, `port/proto`, several ports on
/// one line, and build-args that only resolve at build time. We take the first
/// literal TCP port and ignore the rest: a preview publishes one port, and
/// guessing which of five a person wants is worse than showing the one we
/// picked and letting them see it.
pub fn exposed_port(dockerfile: &str) -> Option<u16> {
    for line in dockerfile.lines() {
        let line = line.trim();
        // `EXPOSE`, case-insensitively — Dockerfile instructions are not
        // case-sensitive and plenty of real files use lowercase.
        let Some(rest) = line
            .get(..6)
            .filter(|w| w.eq_ignore_ascii_case("expose"))
            .and_then(|_| line.get(6..))
        else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        for token in rest.split_whitespace() {
            // `8080/tcp` -> 8080. A `/udp` port cannot serve a browser, so it
            // is skipped rather than published to no purpose.
            let (number, proto) = match token.split_once('/') {
                Some((n, p)) => (n, Some(p)),
                None => (token, None),
            };
            if proto.is_some_and(|p| !p.eq_ignore_ascii_case("tcp")) {
                continue;
            }
            // `$PORT` and `${PORT}` only resolve at build time; there is
            // nothing honest to do with them here.
            if let Ok(n) = number.parse::<u16>() {
                if n != 0 {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Decide what to do with a branch, given whether it has a Dockerfile and what
/// that file says.
///
/// Takes the text rather than a path so the whole decision is testable without
/// a filesystem — the caller does the one read.
pub fn plan(dockerfile: Option<&str>) -> Result<Port, NoRecipe> {
    let Some(text) = dockerfile else {
        return Err(NoRecipe::NoDockerfile);
    };
    Ok(match exposed_port(text) {
        Some(number) => Port {
            number,
            source: PortSource::Exposed,
        },
        None => Port {
            number: ASSUMED_PORT,
            source: PortSource::Assumed,
        },
    })
}

/// Docker's name for a preview's image and container.
///
/// Prefixed and derived from the row id rather than from the task title: titles
/// are user text, contain spaces and slashes, and are not unique. The id is
/// already both.
pub fn image_tag(preview_id: &uuid::Uuid) -> String {
    format!("aichip-preview:{}", short(preview_id))
}

pub fn container_name(preview_id: &uuid::Uuid) -> String {
    format!("aichip-preview-{}", short(preview_id))
}

/// The hostname label a preview answers to: `fix-the-login-a1b2c3`.
///
/// Built from the card's title so the address bar says what you are looking at,
/// and suffixed with part of the **task** id — not the preview's. That is the
/// whole point: a rebuild makes a new preview row, and if the name came from
/// that row it would change every time, which is exactly the problem with the
/// port that this exists to solve. Keyed to the card, the tab you left open
/// keeps working across rebuilds, wakes and restarts.
///
/// Everything that is not a DNS label character is dropped rather than escaped
/// — a hostname is not a place to round-trip user text.
pub fn slug(title: &str, task_id: &uuid::Uuid) -> String {
    let mut label = String::new();
    let mut last_dash = true;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            label.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !label.is_empty() {
            label.push('-');
            last_dash = true;
        }
    }
    // A DNS label is 63 characters; leaving room for the suffix keeps the whole
    // thing legal without having to think about it again.
    let trimmed: String = label.trim_matches('-').chars().take(40).collect();
    let trimmed = trimmed.trim_end_matches('-');
    let tail = &short(task_id)[..6];
    if trimmed.is_empty() {
        // A title of nothing but punctuation is rare but real, and a hostname
        // starting with `-` is not a hostname.
        format!("preview-{tail}")
    } else {
        format!("{trimmed}-{tail}")
    }
}

fn short(id: &uuid::Uuid) -> String {
    id.simple().to_string()[..12].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_tcp_port_it_can_use() {
        assert_eq!(exposed_port("EXPOSE 80"), Some(80));
        assert_eq!(exposed_port("expose 3000"), Some(3000));
        assert_eq!(exposed_port("EXPOSE 8080/tcp"), Some(8080));
        // Several on one line: the first wins, and the UI shows which.
        assert_eq!(exposed_port("EXPOSE 8000 9000"), Some(8000));
        // Real files put it after a build stage and a pile of RUN lines.
        assert_eq!(
            exposed_port("FROM node:20\nRUN npm ci\n\nEXPOSE 5173\nCMD [\"npm\",\"start\"]"),
            Some(5173)
        );
    }

    #[test]
    fn skips_what_it_cannot_honestly_publish() {
        // A browser cannot use a UDP port.
        assert_eq!(exposed_port("EXPOSE 53/udp"), None);
        // ...but a UDP port next to a TCP one must not hide the TCP one.
        assert_eq!(exposed_port("EXPOSE 53/udp 8080/tcp"), Some(8080));
        // Build-args resolve at build time, not here.
        assert_eq!(exposed_port("EXPOSE $PORT"), None);
        assert_eq!(exposed_port("EXPOSE ${PORT}"), None);
        assert_eq!(exposed_port("EXPOSE 70000"), None);
        assert_eq!(exposed_port("EXPOSE 0"), None);
        assert_eq!(exposed_port(""), None);
    }

    #[test]
    fn does_not_mistake_other_words_for_expose() {
        // The realistic false positive: a comment, or a word that starts the
        // same way. Both would publish a port nobody asked for.
        assert_eq!(exposed_port("# EXPOSE 80"), None);
        assert_eq!(exposed_port("EXPOSED 80"), None);
        assert_eq!(exposed_port("RUN echo EXPOSE 80"), None);
        assert_eq!(exposed_port("EXPOSE80"), None);
    }

    #[test]
    fn a_missing_dockerfile_says_so_rather_than_guessing() {
        assert_eq!(plan(None), Err(NoRecipe::NoDockerfile));
    }

    #[test]
    fn a_dockerfile_with_no_expose_is_a_guess_that_admits_it() {
        // The important half is the source, not the number: the UI uses it to
        // say "assumed", so a blank page has an explanation attached.
        assert_eq!(
            plan(Some("FROM nginx")),
            Ok(Port {
                number: ASSUMED_PORT,
                source: PortSource::Assumed
            })
        );
        assert_eq!(
            plan(Some("FROM nginx\nEXPOSE 8080")),
            Ok(Port {
                number: 8080,
                source: PortSource::Exposed
            })
        );
    }

    #[test]
    fn the_name_is_keyed_to_the_card_so_a_rebuild_keeps_it() {
        // The regression this exists for: deriving from the preview row made
        // the name change on every rebuild, which is the problem ports already
        // had. Two different previews of the same card must share a name.
        let task = uuid::Uuid::parse_str("0191f3c2-1111-7000-8000-abcdefabcdef").unwrap();
        assert_eq!(slug("Fix the login", &task), slug("Fix the login", &task));
        let other = uuid::Uuid::parse_str("0191aaaa-2222-7000-8000-abcdefabcdef").unwrap();
        assert_ne!(slug("Fix the login", &task), slug("Fix the login", &other));
    }

    #[test]
    fn slugs_are_legal_hostname_labels() {
        let id = uuid::Uuid::parse_str("0191f3c2-1111-7000-8000-abcdefabcdef").unwrap();
        assert_eq!(slug("Fix the login", &id), "fix-the-login-0191f3");
        // Case, punctuation and runs of separators all collapse.
        assert_eq!(slug("Fix  THE   login!!", &id), "fix-the-login-0191f3");
        assert_eq!(slug("feat/add-oauth", &id), "feat-add-oauth-0191f3");
        // Leading and trailing separators would make an illegal label.
        assert_eq!(slug("  -- trim me -- ", &id), "trim-me-0191f3");
        // A title with nothing usable in it still has to produce a hostname.
        assert_eq!(slug("!!! ???", &id), "preview-0191f3");
        assert_eq!(slug("", &id), "preview-0191f3");
        // Non-ASCII is dropped rather than transliterated; the id still makes
        // it addressable.
        assert_eq!(slug("日本語", &id), "preview-0191f3");
        // Long titles stay inside a DNS label.
        let long = slug(&"a".repeat(200), &id);
        assert!(long.len() < 63, "{} chars", long.len());
        for s in [slug("Fix the login", &id), long] {
            assert!(s
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
            assert!(!s.starts_with('-') && !s.ends_with('-'));
        }
    }

    #[test]
    fn names_are_derived_from_the_id_not_from_user_text() {
        let id = uuid::Uuid::parse_str("0191f3c2-1111-7000-8000-abcdefabcdef").unwrap();
        assert_eq!(container_name(&id), "aichip-preview-0191f3c21111");
        assert_eq!(image_tag(&id), "aichip-preview:0191f3c21111");
        // Docker rejects names with most punctuation; this shape has none.
        assert!(container_name(&id)
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }
}
