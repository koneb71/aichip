//! The documents say what the code does — checked, so they keep saying it.
//!
//! A document that lags the code is worse than none: it is believed. These
//! read the code for the facts a reader most needs to be current — which
//! engines exist, what an engine can declare about itself, what can be set in
//! the environment, what each migration was for — and fail when a document
//! does not mention one. They do not check that the prose is *right*; review
//! does that. They check that nothing new arrives without any prose at all.
//!
//! Here rather than in `eren-shared` because the list of engines is this
//! crate's `real_engines`, and asking it is the only way the test cannot drift
//! from what `serve` actually registers.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|e| panic!("could not read {relative}: {e}"))
}

/// `wanted` items missing from `doc`, each looked for as `format(item)`.
fn missing<'a>(
    doc: &str,
    wanted: impl IntoIterator<Item = &'a String>,
    format: impl Fn(&str) -> String,
) -> Vec<String> {
    wanted
        .into_iter()
        .map(|w| format(w))
        .filter(|w| !doc.contains(w.as_str()))
        .collect()
}

/// Every `.rs` file under `crates/`.
fn sources() -> Vec<(PathBuf, String)> {
    let mut out = vec![];
    let mut stack = vec![root().join("crates")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                out.push((path, text));
            }
        }
    }
    out
}

fn engine_ids() -> Vec<String> {
    super::real_engines(super::LocalHosts::default())
        .iter()
        .map(|e| e.id().to_string())
        .collect()
}

/// The fields of `Capabilities`, read from its declaration.
fn capability_fields() -> Vec<String> {
    let lib = read("crates/eren-engines/src/lib.rs");
    let start = lib
        .find("pub struct Capabilities {")
        .expect("Capabilities is declared in eren-engines/src/lib.rs");
    lib[start..]
        .lines()
        .skip(1)
        .take_while(|l| !l.starts_with('}'))
        .filter_map(|l| l.trim().strip_prefix("pub ")?.split_once(':'))
        .map(|(name, _)| name.trim().to_string())
        .collect()
}

/// Every key read through `brand::var` / `brand::var_os`, without its prefix.
fn env_keys() -> Vec<String> {
    let mut keys = vec![];
    for (_, text) in sources() {
        for call in ["brand::var(\"", "brand::var_os(\""] {
            for (at, _) in text.match_indices(call) {
                let rest = &text[at + call.len()..];
                if let Some(end) = rest.find('"') {
                    keys.push(rest[..end].to_string());
                }
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

/// The four-digit number of every migration.
fn migration_numbers() -> Vec<String> {
    let mut numbers: Vec<String> = std::fs::read_dir(root().join("crates/eren-core/migrations"))
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.ends_with(".sql").then(|| name[..4].to_string())
        })
        .collect();
    numbers.sort();
    numbers
}

#[test]
fn every_engine_is_in_the_readme_and_claude_md() {
    let ids = engine_ids();
    assert!(ids.len() >= 9, "{ids:?}");
    for doc in ["README.md", "CLAUDE.md"] {
        let gaps = missing(&read(doc), &ids, |id| format!("`{id}`"));
        assert!(gaps.is_empty(), "{doc} does not mention engine(s) {gaps:?}");
    }
}

#[test]
fn every_capability_is_explained_in_claude_md() {
    let fields = capability_fields();
    // Sanity: the parse found the struct, not nothing.
    assert!(
        fields.contains(&"interactive_permissions".to_string()),
        "{fields:?}"
    );
    let gaps = missing(&read("CLAUDE.md"), &fields, |f| format!("`{f}`"));
    assert!(
        gaps.is_empty(),
        "CLAUDE.md does not explain capabilities {gaps:?}"
    );
}

#[test]
fn every_setting_is_in_env_example_and_the_readme() {
    let keys = env_keys();
    assert!(keys.contains(&"BIND".to_string()), "{keys:?}");
    for doc in [".env.example", "README.md"] {
        let gaps = missing(&read(doc), &keys, eren_shared::brand::env_name);
        assert!(gaps.is_empty(), "{doc} does not mention {gaps:?}");
    }
}

#[test]
fn every_migration_has_an_architecture_note() {
    let numbers = migration_numbers();
    assert!(numbers.len() >= 88, "{numbers:?}");
    let gaps = missing(&read("docs/architecture.md"), &numbers, str::to_string);
    assert!(
        gaps.is_empty(),
        "docs/architecture.md says nothing about migration(s) {gaps:?}"
    );
}
