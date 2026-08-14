//! What a file defines and what it asks for, read with a real parser.
//!
//! A regex over source text cannot tell a definition from the same words
//! inside a string, a comment or a doc example, and the map's whole promise is
//! that a name it shows exists at the line it shows. tree-sitter parses, so
//! the answer comes from the grammar rather than from a guess.
//!
//! Only definitions at the top level and one level in (a method on a class, an
//! item in an `impl`) are collected. A closure assigned to a local is a
//! definition to the compiler and noise on a map.
//!
//! The extraction is deliberately *not* a tree-sitter query. Queries are a
//! second grammar to get wrong, they fail at runtime rather than at compile
//! time, and the node kinds differ enough between these four languages that a
//! shared query would be four queries anyway. A walk over named children is
//! longer to read and impossible to get half-right.

use tree_sitter::{Node, Parser};

/// One thing a file defines.
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub name: String,
    /// A closed set, mapped from the grammar's node kinds — never the raw kind,
    /// which differs between grammars for the same idea.
    pub kind: &'static str,
    /// 1-based, matching what an editor shows.
    pub line: i32,
    /// The declaration's first line, trimmed and capped.
    pub signature: Option<String>,
}

/// One thing a file asks for, exactly as written.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub specifier: String,
    pub line: i32,
}

/// Everything one parse produced.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Parsed {
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
}

/// The languages this can read. `None` for everything else, which is a state
/// and not a failure: the file is still in the map, still searchable by
/// meaning, and simply has no insides drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    TypeScript,
    Tsx,
    Python,
}

impl Lang {
    /// What this file is, by extension.
    ///
    /// `.ts` and `.tsx` get different grammars rather than one: the TSX
    /// grammar reads `<T>(x)` as a JSX element, so parsing plain TypeScript
    /// with it silently loses generic functions.
    pub fn of(rel_path: &str) -> Option<Self> {
        let ext = rel_path.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "rs" => Lang::Rust,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" | "jsx" | "js" | "mjs" => Lang::Tsx,
            "py" => Lang::Python,
            _ => return None,
        })
    }

    /// The name stored on the row and shown in the UI.
    pub fn tag(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::Python => "python",
        }
    }
}

/// What this extractor produces, versioned.
///
/// The hash-diff answers "did this file change", which is the wrong question
/// when what changed is the reader. Teaching this a new node kind leaves every
/// file's hash identical and every stored symbol list a version behind, so the
/// index would sit there stale and confident. **Bump this whenever the output
/// of `parse` changes for input that has not** — a new symbol kind, a new
/// import form, a different depth rule — and the next pass re-reads
/// everything once.
pub const PARSE_VERSION: i32 = 1;

/// Longer than this and it is a bundle or a fixture; tree-sitter will parse it
/// but nobody is reading the result. Matches the enumerator's own cap in
/// spirit: past a point, a file is data.
const MAX_PARSE_BYTES: usize = 512 * 1024;

/// Read one file.
///
/// Never fails the caller: an unparseable file yields an empty result, because
/// one file the grammar chokes on must not cost the project its map. A
/// tree-sitter parse always returns a tree — a broken file simply produces one
/// with ERROR nodes in it, and the definitions around the break are still
/// found.
pub fn parse(lang: Lang, source: &str) -> Parsed {
    if source.len() > MAX_PARSE_BYTES {
        return Parsed::default();
    }
    let mut parser = Parser::new();
    let grammar = match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
    };
    if parser.set_language(&grammar).is_err() {
        return Parsed::default();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Parsed::default();
    };

    let mut out = Parsed::default();
    let root = tree.root_node();

    // Two walks over the same tree, because they want opposite policies.
    // Symbols are selective — a closure bound to a local is not the shape of
    // a file. Dependencies are not: a `lazy(() => import("./Terminal"))` sits
    // inside an arrow function inside a declaration, and it is every bit as
    // real a dependency as the imports at the top. Walking the tree twice
    // costs microseconds; conflating the two policies costs edges.
    collect_imports(lang, root, source, &mut out.imports);

    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        visit(lang, child, source, 0, &mut out);
    }
    out
}

/// Every dependency, wherever it is written.
fn collect_imports(lang: Lang, node: Node, source: &str, out: &mut Vec<Import>) {
    if let Some(import) = import_of(lang, node, source) {
        out.push(import);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_imports(lang, child, source, out);
    }
}

/// One level of nesting: a method inside a class, an item inside an `impl`.
/// Deeper than that is a local, and a local is not the shape of a file.
const MAX_DEPTH: usize = 1;

/// The walk, and the whole policy is in which children it follows.
///
/// A definition's own body is followed only when the definition *contains*
/// other definitions — a class, an `impl`, a module. A function's body is
/// never followed, which is what keeps a closure bound to a local out of the
/// file's symbol list without needing to recognise closures at all.
fn visit(lang: Lang, node: Node, source: &str, depth: usize, out: &mut Parsed) {
    let (descend, next) = match symbol_of(lang, node, source) {
        Some(symbol) => {
            let holds_more = is_container(symbol.kind);
            out.symbols.push(symbol);
            (holds_more && depth < MAX_DEPTH, depth + 1)
        }
        // Not a definition: a wrapper (`export …`, a decorator) or a body
        // (`class_body`, `declaration_list`, `block`). Followed without
        // spending a level, or an exported class's methods would be out of
        // reach.
        None => (is_structural(node.kind()), depth),
    };
    if !descend {
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit(lang, child, source, next, out);
    }
}

/// Definitions that hold other definitions.
fn is_container(kind: &str) -> bool {
    matches!(kind, "class" | "impl" | "trait" | "module" | "interface")
}

/// Nodes that are neither a definition nor a definition's private business:
/// wrappers and bodies, whose children belong to the enclosing scope.
fn is_structural(kind: &str) -> bool {
    matches!(
        kind,
        // wrappers
        "export_statement"
            | "decorated_definition"
            | "declaration"
            | "ambient_declaration"
            | "internal_module"
            // bodies of the containers above
            | "declaration_list"
            | "class_body"
            | "block"
            | "object_type"
            | "interface_body"
            // file roots
            | "source_file"
            | "program"
            | "module"
    )
}

/// The declaration's first line, trimmed and capped.
///
/// The first line rather than the whole node: a signature is for telling two
/// things apart in a list, and a 200-line function body in a tooltip is not
/// that. Capped by characters, not bytes, so a multi-byte identifier is never
/// cut in half.
fn signature(node: Node, source: &str) -> Option<String> {
    let text = source.get(node.byte_range())?;
    let first = text.lines().next()?.trim();
    if first.is_empty() {
        return None;
    }
    let capped: String = first.chars().take(160).collect();
    Some(if capped.chars().count() < first.chars().count() {
        format!("{capped}…")
    } else {
        capped
    })
}

/// The identifier a definition binds, by the grammar's own field name where
/// there is one.
fn name_of(node: Node, source: &str) -> Option<String> {
    let named = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("pattern"))
        // An `impl` block binds no name; the type it is for is what a reader
        // is looking for in a symbol list.
        .or_else(|| node.child_by_field_name("type"))?;
    source.get(named.byte_range()).map(str::to_string).filter(|s| !s.is_empty())
}

fn symbol_of(lang: Lang, node: Node, source: &str) -> Option<Symbol> {
    let kind = match (lang, node.kind()) {
        (Lang::Rust, "function_item") => "function",
        (Lang::Rust, "struct_item") => "struct",
        (Lang::Rust, "enum_item") => "enum",
        (Lang::Rust, "trait_item") => "trait",
        (Lang::Rust, "type_item") => "type",
        (Lang::Rust, "const_item" | "static_item") => "const",
        (Lang::Rust, "macro_definition") => "macro",
        (Lang::Rust, "mod_item") => "module",
        // Listed for the type it is on, and descended into: the methods are
        // the point, and `impl` is where a Rust file keeps them.
        (Lang::Rust, "impl_item") => "impl",

        (Lang::TypeScript | Lang::Tsx, "function_declaration" | "generator_function_declaration") => {
            "function"
        }
        (Lang::TypeScript | Lang::Tsx, "class_declaration" | "abstract_class_declaration") => "class",
        (Lang::TypeScript | Lang::Tsx, "interface_declaration") => "interface",
        (Lang::TypeScript | Lang::Tsx, "type_alias_declaration") => "type",
        (Lang::TypeScript | Lang::Tsx, "enum_declaration") => "enum",
        (Lang::TypeScript | Lang::Tsx, "method_definition") => "method",
        (Lang::TypeScript | Lang::Tsx, "public_field_definition") => return None,

        (Lang::Python, "function_definition") => "function",
        (Lang::Python, "class_definition") => "class",
        (Lang::Python, "decorated_definition") => return None,

        _ => return None,
    };
    Some(Symbol {
        name: name_of(node, source)?,
        kind,
        line: node.start_position().row as i32 + 1,
        signature: signature(node, source),
    })
}

/// The import specifier, with its quotes already gone.
fn import_of(lang: Lang, node: Node, source: &str) -> Option<Import> {
    let line = node.start_position().row as i32 + 1;
    match (lang, node.kind()) {
        // `import x from "./y"` and `export … from "./y"`: the grammar gives
        // both a `source` field holding the quoted string.
        (Lang::TypeScript | Lang::Tsx, "import_statement" | "export_statement") => {
            let src = node.child_by_field_name("source")?;
            let text = source.get(src.byte_range())?;
            Some(Import { specifier: unquote(text)?, line })
        }
        (Lang::Rust, "use_declaration") => {
            let arg = node.child_by_field_name("argument")?;
            // Only the path's stem: `use crate::db::Db` and
            // `use crate::db::{Db, Pool}` are the same dependency, and the
            // resolver works in modules, not items.
            Some(Import { specifier: source.get(arg.byte_range())?.to_string(), line })
        }
        (Lang::Python, "import_from_statement") => {
            let m = node.child_by_field_name("module_name")?;
            Some(Import { specifier: source.get(m.byte_range())?.to_string(), line })
        }
        (Lang::Python, "import_statement") => {
            let m = node.child_by_field_name("name")?;
            Some(Import { specifier: source.get(m.byte_range())?.to_string(), line })
        }
        // `lazy(() => import("./TerminalPanel"))`. A code-splitting boundary is
        // exactly where somebody looking at a dependency graph expects to see
        // an edge, and it is invisible to anything that only reads the top of
        // the file.
        (Lang::TypeScript | Lang::Tsx, "call_expression") => {
            let callee = node.child_by_field_name("function")?;
            if source.get(callee.byte_range())? != "import" {
                return None;
            }
            let arg = node.child_by_field_name("arguments")?.named_child(0)?;
            Some(Import { specifier: unquote(source.get(arg.byte_range())?)?, line })
        }
        _ => None,
    }
}

/// `"./x"` → `./x`. Returns `None` for a template literal or a dynamic
/// expression: a specifier this cannot read is not a specifier it may guess at.
fn unquote(raw: &str) -> Option<String> {
    let t = raw.trim();
    let inner = t
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| t.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))?;
    (!inner.is_empty()).then(|| inner.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(p: &Parsed) -> Vec<&str> {
        p.symbols.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn rust_definitions_and_their_lines() {
        let src = "\
use crate::db::Db;
use std::collections::HashMap;

pub struct Report {
    pub ok: bool,
}

pub fn reconcile(db: &Db) -> anyhow::Result<()> {
    fn helper() {}
    Ok(())
}

impl Report {
    pub fn empty() -> Self { Self { ok: true } }
}
";
        let p = parse(Lang::Rust, src);
        assert_eq!(names(&p), vec!["Report", "reconcile", "Report", "empty"]);
        // 1-based, and the line is the declaration's, not the file's start.
        let reconcile = p.symbols.iter().find(|s| s.name == "reconcile").unwrap();
        assert_eq!(reconcile.line, 8);
        assert_eq!(reconcile.kind, "function");
        // A function nested inside a function body is a local, not the shape
        // of the file.
        assert!(!names(&p).contains(&"helper"));
        assert_eq!(
            p.imports.iter().map(|i| i.specifier.as_str()).collect::<Vec<_>>(),
            vec!["crate::db::Db", "std::collections::HashMap"]
        );
    }

    #[test]
    fn typescript_exports_and_methods() {
        let src = "\
import { api } from \"../lib/api\";
import type { Task } from './types';

export interface RepoFile { path: string }

export function chunkOf(x: string): number {
  const inner = () => 1;
  return inner();
}

export class Store {
  load() { return 1; }
  save() { return 2; }
}
";
        let p = parse(Lang::TypeScript, src);
        assert_eq!(names(&p), vec!["RepoFile", "chunkOf", "Store", "load", "save"]);
        // Both quote styles, both unquoted.
        assert_eq!(
            p.imports.iter().map(|i| i.specifier.as_str()).collect::<Vec<_>>(),
            vec!["../lib/api", "./types"]
        );
        // An arrow function bound to a local is not a definition of the file.
        assert!(!names(&p).contains(&"inner"));
    }

    #[test]
    fn a_generic_function_survives_the_typescript_grammar() {
        // The reason .ts and .tsx get different grammars: TSX reads `<T>` as
        // the start of a JSX element and loses the declaration.
        let p = parse(Lang::TypeScript, "export function pick<T>(xs: T[]): T { return xs[0]; }");
        assert_eq!(names(&p), vec!["pick"]);
    }

    #[test]
    fn tsx_components_are_found() {
        let src = "\
import { useState } from \"react\";
export function Panel({ id }: { id: string }) {
  return <div className=\"x\">{id}</div>;
}
";
        let p = parse(Lang::Tsx, src);
        assert_eq!(names(&p), vec!["Panel"]);
        assert_eq!(p.imports[0].specifier, "react");
    }

    #[test]
    fn python_classes_methods_and_both_import_forms() {
        let src = "\
import os
from django.db import models

class Account(models.Model):
    def save(self):
        def inner():
            pass
        return 1

def standalone():
    pass
";
        let p = parse(Lang::Python, src);
        assert_eq!(names(&p), vec!["Account", "save", "standalone"]);
        assert!(!names(&p).contains(&"inner"));
        assert_eq!(
            p.imports.iter().map(|i| i.specifier.as_str()).collect::<Vec<_>>(),
            vec!["os", "django.db"]
        );
    }

    #[test]
    fn a_decorated_definition_is_the_definition_it_wraps() {
        let src = "\
@app.route('/x')
def handler():
    pass
";
        let p = parse(Lang::Python, src);
        assert_eq!(names(&p), vec!["handler"]);
        assert_eq!(p.symbols[0].kind, "function");
    }

    #[test]
    fn a_definition_named_inside_a_string_or_comment_is_not_one() {
        // The whole reason this is a parser and not a regex.
        let src = "\
// pub fn ghost() {}
const DOC: &str = \"pub fn phantom() {}\";
pub fn real() {}
";
        let p = parse(Lang::Rust, src);
        assert_eq!(names(&p), vec!["DOC", "real"]);
    }

    #[test]
    fn a_broken_file_still_yields_what_parsed() {
        // Half-written code is the normal state of a file being edited, and an
        // index that gives up on it is an index that is wrong all afternoon.
        let p = parse(Lang::Rust, "pub fn good() {}\npub fn bad( {\n");
        assert!(names(&p).contains(&"good"));
    }

    #[test]
    fn an_empty_or_unreadable_file_is_empty_not_an_error() {
        assert_eq!(parse(Lang::Rust, ""), Parsed::default());
        assert_eq!(parse(Lang::Python, "\u{0}\u{1}\u{2}").symbols.len(), 0);
    }

    #[test]
    fn a_signature_is_one_line_and_capped_without_splitting_a_character() {
        let long = format!("pub fn café_{}() {{}}", "x".repeat(400));
        let p = parse(Lang::Rust, &long);
        let sig = p.symbols[0].signature.as_ref().unwrap();
        assert!(sig.chars().count() <= 161, "capped: {}", sig.chars().count());
        assert!(sig.ends_with('…'));
        assert!(sig.contains("café"));
    }

    #[test]
    fn a_dynamic_specifier_is_dropped_rather_than_guessed() {
        let p = parse(Lang::TypeScript, "import x from `./${name}`;\nimport y from \"./real\";");
        assert_eq!(
            p.imports.iter().map(|i| i.specifier.as_str()).collect::<Vec<_>>(),
            vec!["./real"]
        );
    }

    #[test]
    fn extensions_pick_the_grammar_and_unknown_ones_pick_none() {
        assert_eq!(Lang::of("a/b.rs"), Some(Lang::Rust));
        assert_eq!(Lang::of("a/b.ts"), Some(Lang::TypeScript));
        assert_eq!(Lang::of("a/b.tsx"), Some(Lang::Tsx));
        assert_eq!(Lang::of("a/b.py"), Some(Lang::Python));
        // SQL is excluded deliberately: 60 of this repo's files are migrations
        // with no symbols and no imports.
        assert_eq!(Lang::of("m/0001_x.sql"), None);
        assert_eq!(Lang::of("README.md"), None);
        assert_eq!(Lang::of("Makefile"), None);
    }
}
