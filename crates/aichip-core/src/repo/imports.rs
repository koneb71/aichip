//! Turning what a file asked for into a file that exists.
//!
//! Parsing a specifier is easy and resolving one is where every bug lives, so
//! the two are separate: `symbols::parse` stores `"../lib/api"` verbatim and
//! this decides, against the project's actual file list, that it means
//! `web/src/lib/api.ts`. Resolution is redone on every pass rather than frozen
//! at parse time, because adding one file can make a previously dangling
//! specifier point somewhere.
//!
//! **An unresolvable specifier returns `None`, never a guess.** A wrong edge is
//! worse than a missing one: a missing edge understates coupling, which a
//! reader can discover, while a wrong edge sends somebody to change a file
//! that has nothing to do with the one they are looking at. Imports of `react`
//! and `std::collections` resolve to nothing here and are dropped rather than
//! drawn as nodes that do not exist.

use std::collections::{HashMap, HashSet};

/// The project's files, in the shape resolution needs.
pub struct PathSet {
    exact: HashSet<String>,
    all: Vec<String>,
    /// Symbol name → the files that define it. Only needed by Rust, and only
    /// for the case below, but measured on this repository it is the
    /// difference between a graph and a scatter: `use crate::AppState`
    /// appears 36 times and `use crate::Db` 8, and a resolver that knows only
    /// about modules answers `None` to every one of them. That left 88 of 287
    /// files looking isolated when only 10 truly are — and an isolated node
    /// reads as "safe to delete".
    defines: HashMap<String, Vec<String>>,
}

impl PathSet {
    pub fn new(paths: &[String]) -> Self {
        Self {
            exact: paths.iter().cloned().collect(),
            all: paths.to_vec(),
            defines: HashMap::new(),
        }
    }

    /// Teach the resolver what each file defines.
    pub fn with_symbols(mut self, defines: HashMap<String, Vec<String>>) -> Self {
        self.defines = defines;
        self
    }

    fn has(&self, p: &str) -> bool {
        self.exact.contains(p)
    }

    /// The one file under `within` that defines `name`, or nothing.
    ///
    /// Scoped to a directory prefix so `crate::AppState` cannot resolve into a
    /// different crate that happens to define the same name. Two definitions
    /// inside the scope is genuine ambiguity and resolves to `None`.
    fn defining(&self, name: &str, within: &str) -> Option<&str> {
        let mut found: Option<&str> = None;
        for p in self.defines.get(name)? {
            if !p.starts_with(within) {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(p);
        }
        found
    }

    /// The one file whose path ends in `tail`, or nothing.
    ///
    /// This is what makes an aliased or absolute specifier resolvable without
    /// reading a tsconfig: `@/lib/api` names a path ending in `lib/api.ts`, and
    /// if exactly one file in the project does, that is not a guess. Two
    /// candidates means the specifier is genuinely ambiguous from here, and
    /// ambiguity resolves to `None`.
    fn unique_ending(&self, tail: &str) -> Option<&str> {
        let mut found: Option<&str> = None;
        for p in &self.all {
            let hit = p == tail || p.ends_with(&format!("/{tail}"));
            if hit {
                if found.is_some() {
                    return None;
                }
                found = Some(p);
            }
        }
        found
    }
}

/// Extensions tried for an extensionless TypeScript specifier, in the order a
/// bundler tries them.
const TS_EXTENSIONS: [&str; 7] = ["ts", "tsx", "js", "jsx", "mjs", "cjs", "d.ts"];

/// Resolve one specifier to a file in this project, or to nothing.
///
/// `from` is the importing file's repo-relative path; the language is taken
/// from it, because a `crate::` specifier only means anything in Rust and a
/// `./x` means different things in Python and TypeScript.
pub fn resolve(specifier: &str, from: &str, set: &PathSet) -> Option<String> {
    let spec = specifier.trim();
    if spec.is_empty() {
        return None;
    }
    match super::symbols::Lang::of(from)? {
        super::symbols::Lang::Rust => resolve_rust(spec, from, set),
        super::symbols::Lang::TypeScript | super::symbols::Lang::Tsx => resolve_ts(spec, from, set),
        super::symbols::Lang::Python => resolve_python(spec, from, set),
    }
}

/// `web/src/lib/api.ts` → `web/src/lib`.
fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// Apply `.` and `..` segments. Returns `None` for a path that climbs above the
/// repository root — outside the project is not in the project.
fn normalize(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = if base.is_empty() {
        vec![]
    } else {
        base.split('/').collect()
    };
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

fn resolve_ts(spec: &str, from: &str, set: &PathSet) -> Option<String> {
    // A bare package name is a dependency, not a file here. `@scope/pkg` looks
    // like an alias and is not one; `@/x` is the alias convention.
    let candidate = if spec.starts_with("./") || spec.starts_with("../") {
        normalize(dir_of(from), spec)?
    } else if let Some(rest) = spec.strip_prefix("@/") {
        // Aliased to some root this cannot read without a tsconfig. The unique
        // ending settles it, or nothing does.
        return try_ts_endings(rest, set);
    } else if spec.starts_with('.') || spec.starts_with('/') {
        spec.trim_start_matches('/').to_string()
    } else {
        return None;
    };
    try_ts_candidate(&candidate, set)
}

/// A TypeScript specifier names a module, and a module is one of several files.
fn try_ts_candidate(candidate: &str, set: &PathSet) -> Option<String> {
    if set.has(candidate) {
        return Some(candidate.to_string());
    }
    // `import "./foo.js"` in TypeScript source means `./foo.ts` — the ESM
    // convention of writing the emitted extension.
    if let Some(stem) = candidate
        .strip_suffix(".js")
        .or_else(|| candidate.strip_suffix(".jsx"))
    {
        for ext in TS_EXTENSIONS {
            let p = format!("{stem}.{ext}");
            if set.has(&p) {
                return Some(p);
            }
        }
    }
    for ext in TS_EXTENSIONS {
        let p = format!("{candidate}.{ext}");
        if set.has(&p) {
            return Some(p);
        }
    }
    for ext in TS_EXTENSIONS {
        let p = format!("{candidate}/index.{ext}");
        if set.has(&p) {
            return Some(p);
        }
    }
    None
}

/// The alias case: the same module shapes, matched by unique ending.
fn try_ts_endings(rest: &str, set: &PathSet) -> Option<String> {
    for ext in TS_EXTENSIONS {
        if let Some(hit) = set.unique_ending(&format!("{rest}.{ext}")) {
            return Some(hit.to_string());
        }
    }
    for ext in TS_EXTENSIONS {
        if let Some(hit) = set.unique_ending(&format!("{rest}/index.{ext}")) {
            return Some(hit.to_string());
        }
    }
    None
}

/// The directory that is this crate's module root: everything up to and
/// including `src`. `crates/aichip-core/src/repo/index.rs` → `crates/aichip-core/src`.
fn crate_root(from: &str) -> Option<&str> {
    let idx = from.rfind("/src/")?;
    Some(&from[..idx + 4])
}

/// A Rust module path names a file two ways, and both have to be tried:
/// `foo.rs` beside its siblings, or `foo/mod.rs`.
fn rust_module(base: &str, module_path: &str, set: &PathSet) -> Option<String> {
    let rel = module_path.replace("::", "/");
    let stem = if base.is_empty() {
        rel
    } else {
        format!("{base}/{rel}")
    };
    for p in [format!("{stem}.rs"), format!("{stem}/mod.rs")] {
        if set.has(&p) {
            return Some(p);
        }
    }
    // `use crate::db::Db` names an item inside `db`, not a module called `Db`.
    // Dropping the last segment is the difference between resolving most Rust
    // imports and resolving almost none.
    let parent = stem.rsplit_once('/')?.0.to_string();
    for p in [format!("{parent}.rs"), format!("{parent}/mod.rs")] {
        if set.has(&p) {
            return Some(p);
        }
    }
    None
}

/// The last resort for a Rust path: it names a type or a function, not a
/// module, so ask what defines it.
///
/// `use crate::AppState` points at whichever file in the crate declares
/// `AppState`, which is what a reader following the import would find. A brace
/// list is not attempted — `use crate::{a, b}` names two things and would need
/// two edges, and the module form above already resolves the common case.
fn rust_item(path: &str, root: &str, set: &PathSet) -> Option<String> {
    let last = path.rsplit("::").next()?.trim();
    if last.is_empty() || last.contains(['{', '*', ' ']) {
        return None;
    }
    set.defining(last, root).map(str::to_string)
}

fn resolve_rust(spec: &str, from: &str, set: &PathSet) -> Option<String> {
    let root = crate_root(from)?;
    // The importing file's own module directory: `foo/bar.rs` is module
    // `foo::bar`, so its `super` is the directory `foo`, and `self` is `bar`'s
    // own directory only when the file is a `mod.rs`.
    let own_dir = dir_of(from);

    if let Some(rest) = spec.strip_prefix("crate::") {
        return rust_module(root, rest, set).or_else(|| rust_item(rest, root, set));
    }
    if let Some(rest) = spec.strip_prefix("self::") {
        return rust_module(own_dir, rest, set);
    }
    if let Some(rest) = spec.strip_prefix("super::") {
        // Each further `super::` climbs one directory. `mod.rs` is its
        // directory's module, so from a `mod.rs` the first `super` is already
        // the parent.
        let mut dir = if from.ends_with("/mod.rs") {
            dir_of(own_dir)
        } else {
            own_dir
        };
        let mut rest = rest;
        while let Some(more) = rest.strip_prefix("super::") {
            dir = dir_of(dir);
            rest = more;
        }
        return rust_module(dir, rest, set);
    }
    // Another crate in this workspace: `aichip_shared::env_guard` lives at
    // `crates/aichip-shared/src/env_guard.rs`. Crate names use `_` in code and
    // `-` in directories, which is the only translation needed.
    let (head, rest) = spec.split_once("::")?;
    if matches!(head, "std" | "core" | "alloc") {
        return None;
    }
    let dir_name = head.replace('_', "-");
    for base in [format!("crates/{dir_name}/src"), format!("{dir_name}/src")] {
        if let Some(hit) = rust_module(&base, rest, set) {
            return Some(hit);
        }
    }
    None
}

fn resolve_python(spec: &str, from: &str, set: &PathSet) -> Option<String> {
    // `from .models import X` / `from ..utils import Y`: leading dots climb.
    let dots = spec.chars().take_while(|c| *c == '.').count();
    if dots > 0 {
        let rest = spec.trim_start_matches('.').replace('.', "/");
        let mut dir = dir_of(from);
        for _ in 1..dots {
            dir = dir_of(dir);
        }
        let stem = if rest.is_empty() {
            dir.to_string()
        } else if dir.is_empty() {
            rest
        } else {
            format!("{dir}/{rest}")
        };
        return python_module(&stem, set);
    }
    // An absolute module path. Whether it is this project's or a dependency's
    // is answered by whether a file with that ending exists here — `os` and
    // `django.db` do not, `apps.accounts.models` does.
    let tail = spec.replace('.', "/");
    for candidate in [format!("{tail}.py"), format!("{tail}/__init__.py")] {
        if let Some(hit) = set.unique_ending(&candidate) {
            return Some(hit.to_string());
        }
    }
    // `from apps.accounts import models` names the package, and the symbol is
    // the module inside it. Try one segment shorter before giving up.
    let parent = tail.rsplit_once('/')?.0.to_string();
    for candidate in [format!("{parent}.py"), format!("{parent}/__init__.py")] {
        if let Some(hit) = set.unique_ending(&candidate) {
            return Some(hit.to_string());
        }
    }
    None
}

fn python_module(stem: &str, set: &PathSet) -> Option<String> {
    for p in [format!("{stem}.py"), format!("{stem}/__init__.py")] {
        if set.has(&p) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file list shaped like the repositories this actually runs against:
    /// a Rust workspace, a Vite web app, and a Django backend.
    fn project() -> PathSet {
        PathSet::new(
            &[
                "crates/aichip-core/src/db.rs",
                "crates/aichip-core/src/lib.rs",
                "crates/aichip-core/src/repo/mod.rs",
                "crates/aichip-core/src/repo/index.rs",
                "crates/aichip-core/src/repo/chunk.rs",
                "crates/aichip-core/src/rag/embed.rs",
                "crates/aichip-shared/src/env_guard.rs",
                "crates/aichip-server/src/lib.rs",
                "crates/aichip-server/src/routes/mod.rs",
                "crates/aichip-server/src/routes/repo_map.rs",
                "web/src/lib/api.ts",
                "web/src/lib/ws.ts",
                "web/src/components/ui/Icon.tsx",
                "web/src/components/map/RepoMapPanel.tsx",
                "web/src/components/map/index.ts",
                "web/src/pages/ProjectPage.tsx",
                "backend/apps/accounts/models.py",
                "backend/apps/accounts/__init__.py",
                "backend/apps/accounts/views.py",
                "backend/config/settings/base.py",
            ]
            .map(str::to_string)
            .to_vec(),
        )
    }

    fn r(spec: &str, from: &str) -> Option<String> {
        resolve(spec, from, &project())
    }

    #[test]
    fn typescript_relative_specifiers_find_their_extension() {
        assert_eq!(
            r("../../lib/api", "web/src/components/map/RepoMapPanel.tsx"),
            Some("web/src/lib/api.ts".into())
        );
        assert_eq!(
            r("./Icon", "web/src/components/ui/Icon.tsx"),
            Some("web/src/components/ui/Icon.tsx".into())
        );
        // One `..` too few lands somewhere that does not exist, and that is
        // `None` rather than the nearest plausible file.
        assert_eq!(
            r("../lib/api", "web/src/components/map/RepoMapPanel.tsx"),
            None
        );
        assert_eq!(
            r("../../lib/ws", "web/src/components/map/RepoMapPanel.tsx"),
            Some("web/src/lib/ws.ts".into())
        );
    }

    #[test]
    fn a_directory_specifier_finds_its_index_file() {
        assert_eq!(
            r("../components/map", "web/src/pages/ProjectPage.tsx"),
            Some("web/src/components/map/index.ts".into())
        );
    }

    #[test]
    fn the_esm_dot_js_convention_finds_the_typescript_source() {
        assert_eq!(
            r("../lib/api.js", "web/src/pages/ProjectPage.tsx"),
            Some("web/src/lib/api.ts".into())
        );
    }

    #[test]
    fn an_alias_resolves_only_when_exactly_one_file_could_be_meant() {
        assert_eq!(
            r("@/lib/api", "web/src/pages/ProjectPage.tsx"),
            Some("web/src/lib/api.ts".into())
        );
        // Nothing in the project ends this way.
        assert_eq!(r("@/lib/nope", "web/src/pages/ProjectPage.tsx"), None);
    }

    #[test]
    fn a_package_is_not_a_file_in_this_project() {
        for spec in ["react", "@xyflow/react", "framer-motion", "js-yaml"] {
            assert_eq!(r(spec, "web/src/pages/ProjectPage.tsx"), None, "{spec}");
        }
    }

    #[test]
    fn a_specifier_that_climbs_out_of_the_repository_resolves_to_nothing() {
        assert_eq!(r("../../../../../etc/passwd", "web/src/lib/api.ts"), None);
    }

    #[test]
    fn rust_crate_self_and_super() {
        let from = "crates/aichip-core/src/repo/index.rs";
        assert_eq!(
            r("crate::db::Db", from),
            Some("crates/aichip-core/src/db.rs".into())
        );
        // A directory module resolves to its mod.rs.
        assert_eq!(
            r("crate::repo", from),
            Some("crates/aichip-core/src/repo/mod.rs".into())
        );
        assert_eq!(
            r("super::chunk", from),
            Some("crates/aichip-core/src/repo/chunk.rs".into())
        );
        assert_eq!(
            r("crate::rag::embed::embed_batch", from),
            Some("crates/aichip-core/src/rag/embed.rs".into())
        );
    }

    #[test]
    fn super_from_a_mod_file_starts_one_level_higher() {
        // `repo/mod.rs` *is* module `repo`, so its `super` is the crate root.
        assert_eq!(
            r("super::db", "crates/aichip-core/src/repo/mod.rs"),
            Some("crates/aichip-core/src/db.rs".into())
        );
    }

    #[test]
    fn another_workspace_crate_is_found_through_its_directory_name() {
        assert_eq!(
            r(
                "aichip_shared::env_guard::is_auth_env",
                "crates/aichip-core/src/repo/index.rs"
            ),
            Some("crates/aichip-shared/src/env_guard.rs".into())
        );
    }

    #[test]
    fn the_standard_library_and_external_crates_are_not_files_here() {
        let from = "crates/aichip-core/src/db.rs";
        for spec in [
            "std::collections::HashMap",
            "tokio::fs",
            "serde::Serialize",
            "anyhow::Result",
        ] {
            assert_eq!(r(spec, from), None, "{spec}");
        }
    }

    #[test]
    fn python_absolute_and_relative_imports() {
        let from = "backend/apps/accounts/views.py";
        assert_eq!(
            r(".models", from),
            Some("backend/apps/accounts/models.py".into())
        );
        assert_eq!(
            r("apps.accounts.models", from),
            Some("backend/apps/accounts/models.py".into())
        );
        // A package resolves to its __init__.
        assert_eq!(
            r("apps.accounts", from),
            Some("backend/apps/accounts/__init__.py".into())
        );
        assert_eq!(r("django.db", from), None);
        assert_eq!(r("os", from), None);
    }

    #[test]
    fn from_package_import_module_finds_the_package_when_the_module_is_the_symbol() {
        // `from apps.accounts import models` parses as module `apps.accounts`.
        assert_eq!(
            r("apps.accounts", "backend/config/settings/base.py"),
            Some("backend/apps/accounts/__init__.py".into())
        );
    }

    #[test]
    fn a_file_in_a_language_with_no_grammar_resolves_nothing() {
        assert_eq!(r("./x", "README.md"), None);
        assert_eq!(r("anything", "migrations/0001_x.sql"), None);
    }

    /// The same project, plus what each file defines.
    fn with_symbols() -> PathSet {
        project().with_symbols(HashMap::from([
            (
                "AppState".to_string(),
                vec!["crates/aichip-server/src/lib.rs".to_string()],
            ),
            (
                "Db".to_string(),
                vec![
                    "crates/aichip-core/src/db.rs".to_string(),
                    // A second definition of the same name in another crate:
                    // the scope is what keeps this from being ambiguous.
                    "crates/aichip-shared/src/env_guard.rs".to_string(),
                ],
            ),
            (
                "Report".to_string(),
                vec![
                    "crates/aichip-core/src/db.rs".to_string(),
                    "crates/aichip-core/src/rag/embed.rs".to_string(),
                ],
            ),
        ]))
    }

    #[test]
    fn a_rust_path_that_names_a_type_resolves_through_the_symbol_table() {
        // `use crate::AppState` is 36 of this repository's imports and names no
        // module at all. Without this it is a dangling edge and the file it
        // came from looks uncoupled.
        let set = with_symbols();
        assert_eq!(
            resolve(
                "crate::AppState",
                "crates/aichip-server/src/routes/repo_map.rs",
                &set
            ),
            Some("crates/aichip-server/src/lib.rs".into())
        );
        // Scoped to the importing crate: the shared crate also defines `Db`,
        // and reaching across for it would be a wrong edge.
        assert_eq!(
            resolve("crate::Db", "crates/aichip-core/src/repo/index.rs", &set),
            Some("crates/aichip-core/src/db.rs".into())
        );
    }

    #[test]
    fn two_definitions_of_a_name_in_one_crate_resolve_to_neither() {
        let set = with_symbols();
        assert_eq!(
            resolve("crate::Report", "crates/aichip-core/src/db.rs", &set),
            None
        );
    }

    #[test]
    fn the_module_form_still_wins_over_the_symbol_table() {
        // `crate::db::Db` names a module and must resolve as one, not through
        // whatever happens to define `Db`.
        let set = with_symbols();
        assert_eq!(
            resolve(
                "crate::db::Db",
                "crates/aichip-core/src/repo/index.rs",
                &set
            ),
            Some("crates/aichip-core/src/db.rs".into())
        );
    }

    #[test]
    fn a_brace_list_or_a_glob_is_not_looked_up_as_a_symbol() {
        let set = with_symbols();
        for spec in ["crate::{AppState, Db}", "crate::routes::*"] {
            let hit = resolve(spec, "crates/aichip-server/src/main.rs", &set);
            assert!(
                hit.is_none() || hit.as_deref() == Some("crates/aichip-server/src/routes/mod.rs"),
                "{spec} resolved to {hit:?}"
            );
        }
    }

    #[test]
    fn an_ambiguous_ending_is_refused_rather_than_picked() {
        let set = PathSet::new(
            &["a/lib/api.ts", "b/lib/api.ts"]
                .map(str::to_string)
                .to_vec(),
        );
        assert_eq!(resolve("@/lib/api", "a/main.ts", &set), None);
    }

    #[test]
    fn resolution_never_returns_a_path_outside_the_project() {
        let set = project();
        for (spec, from) in [
            ("../../../../secrets", "web/src/lib/api.ts"),
            ("crate::nope::nope", "crates/aichip-core/src/db.rs"),
            ("....weird", "backend/apps/accounts/views.py"),
        ] {
            match resolve(spec, from, &set) {
                None => {}
                Some(p) => assert!(set.has(&p), "{spec} from {from} resolved to a non-file {p}"),
            }
        }
    }
}
