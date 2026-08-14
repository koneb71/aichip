//! Split source code into passages worth embedding.
//!
//! Not `rag::chunk`, and the difference is not stylistic. That chunker is
//! shaped for prose: it splits on blank lines, breaks long paragraphs on
//! sentence punctuation, and prefixes every chunk after the first with an `…`
//! and the tail of its predecessor. Every one of those is wrong here. Code is
//! full of blank lines *inside* the unit you want to keep whole, a `.` is a
//! field access rather than a sentence end, and an `…` glued to the front of
//! an excerpt corrupts anything quoted back verbatim — which is the whole
//! point of retrieving source.
//!
//! So: split on top-level boundaries, keep a unit whole where it fits, and
//! carry the line number and the enclosing symbol so a hit can say
//! `api.ts:412 · saveFile` rather than handing back an anonymous paragraph.

/// What a chunk aims for. Smaller than the prose target: a function is a
/// tighter unit of meaning than a paragraph, and a query about one function
/// should not have to out-score three unrelated ones sharing its chunk.
pub const TARGET_CHARS: usize = 900;

/// The hard ceiling. A single enormous function is split rather than skipped —
/// a 4000-line generated file still has content worth finding.
pub const MAX_CHARS: usize = 1800;

/// One passage of source, with enough identity to cite.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeChunk {
    pub content: String,
    /// 1-based, the way an editor counts.
    pub start_line: i32,
    /// The nearest enclosing definition, when a line looked like one.
    pub symbol: Option<String>,
}

/// True when a line begins a top-level definition worth breaking before.
///
/// Indentation is the signal, deliberately: a line that starts at column zero
/// (or one level in, for a method in a class body) and reads like a
/// declaration. This is a heuristic and stays one — tree-sitter replaces it
/// for the symbol table, but chunking wants cheap boundaries, not a parse.
fn starts_definition(line: &str) -> bool {
    let indent = line.len() - line.trim_start().len();
    if indent > 4 {
        return false;
    }
    let t = line.trim_start();
    const OPENERS: [&str; 22] = [
        "fn ", "pub fn ", "async fn ", "pub async fn ", "impl ", "struct ", "enum ", "trait ",
        "mod ", "pub struct ", "pub enum ", "pub trait ", "pub mod ", "def ", "class ",
        "function ", "export ", "const ", "let ", "var ", "type ", "interface ",
    ];
    OPENERS.iter().any(|o| t.starts_with(o))
}

/// The name a definition line declares, for citation.
///
/// Best-effort by design: it takes the first identifier-shaped word after the
/// keyword. A wrong guess costs a slightly worse citation, never a wrong
/// excerpt — the content is carried verbatim either way.
fn symbol_of(line: &str) -> Option<String> {
    let t = line.trim_start();
    let mut words = t.split_whitespace().peekable();
    let mut name: Option<&str> = None;
    while let Some(w) = words.next() {
        let is_keyword = matches!(
            w,
            "pub" | "async" | "export" | "default" | "fn" | "def" | "class" | "function"
                | "struct" | "enum" | "trait" | "mod" | "impl" | "const" | "let" | "var"
                | "type" | "interface" | "static" | "abstract"
        );
        if !is_keyword {
            name = Some(w);
            break;
        }
    }
    let raw = name?;
    // Cut at whatever ends the name: `foo(`, `foo<T>`, `foo:`, `foo=`.
    let cut: String = raw
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    (!cut.is_empty()).then_some(cut)
}

/// Split source into embeddable passages.
///
/// Line-based throughout, so a chunk boundary can never land inside a
/// multi-byte character — the failure `rag::chunk` needed `char_indices` to
/// avoid.
pub fn chunk_code(text: &str) -> Vec<CodeChunk> {
    if text.trim().is_empty() {
        return vec![];
    }
    let lines: Vec<&str> = text.lines().collect();

    let mut out: Vec<CodeChunk> = vec![];
    let mut buf: Vec<&str> = vec![];
    let mut buf_len = 0usize;
    let mut start_line = 1i32;
    // The last definition seen at or before the current chunk's start.
    let mut pending_symbol: Option<String> = None;
    let mut chunk_symbol: Option<String> = None;

    for (i, line) in lines.iter().enumerate() {
        let line_no = i as i32 + 1;
        let is_def = starts_definition(line);
        if is_def {
            pending_symbol = symbol_of(line);
        }

        // Break *before* a definition when the buffer has earned it, so a
        // function and its signature stay in one piece.
        let would_overflow = buf_len + line.len() + 1 > MAX_CHARS;
        let break_here = !buf.is_empty() && ((is_def && buf_len >= TARGET_CHARS) || would_overflow);
        if break_here {
            out.push(CodeChunk {
                content: buf.join("\n"),
                start_line,
                symbol: chunk_symbol.take(),
            });
            buf.clear();
            buf_len = 0;
            start_line = line_no;
        }
        if buf.is_empty() {
            // Whatever definition was in force when this chunk opened.
            chunk_symbol = pending_symbol.clone();
            start_line = line_no;
        }
        buf.push(line);
        buf_len += line.len() + 1;
    }
    if !buf.is_empty() && !buf.join("\n").trim().is_empty() {
        out.push(CodeChunk {
            content: buf.join("\n"),
            start_line,
            symbol: chunk_symbol,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_blank_produce_nothing() {
        assert!(chunk_code("").is_empty());
        assert!(chunk_code("   \n\n  \n").is_empty());
    }

    #[test]
    fn a_short_file_is_one_chunk_starting_at_line_one() {
        let c = chunk_code("fn main() {\n    println!(\"hi\");\n}\n");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].start_line, 1);
        assert_eq!(c[0].symbol.as_deref(), Some("main"));
    }

    #[test]
    fn nothing_ever_exceeds_the_hard_cap() {
        // One enormous function: it must be split, not skipped.
        let body = (0..4000)
            .map(|i| format!("    let x{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let src = format!("pub fn huge() {{\n{body}\n}}\n");
        let chunks = chunk_code(&src);
        assert!(chunks.len() > 1, "a huge function must be split");
        for c in &chunks {
            assert!(
                c.content.len() <= MAX_CHARS,
                "chunk of {} exceeded the cap",
                c.content.len()
            );
        }
    }

    #[test]
    fn a_boundary_never_lands_inside_a_multi_byte_character() {
        // Line-based splitting is what guarantees this; the test pins it.
        let src = (0..400)
            .map(|i| format!("// 日本語のコメント — 行 {i} — ünïcödé"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = chunk_code(&src);
        assert!(chunks.len() > 1);
        // Round-tripping through String proves every chunk is valid UTF-8, and
        // the joined text must still contain the original lines intact.
        for c in &chunks {
            assert!(c.content.contains("日本語"));
        }
    }

    #[test]
    fn line_numbers_are_one_based_and_advance() {
        let src = (0..500)
            .map(|i| format!("const value{i} = {i};"))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = chunk_code(&src);
        assert_eq!(chunks[0].start_line, 1);
        for pair in chunks.windows(2) {
            assert!(
                pair[1].start_line > pair[0].start_line,
                "start_line must advance"
            );
        }
    }

    #[test]
    fn no_chunk_is_prefixed_with_an_overlap_marker() {
        // The prose chunker glues "…" and the previous tail onto every chunk
        // after the first. Source retrieved that way cannot be quoted back.
        let src = (0..300)
            .map(|i| format!("pub fn f{i}() {{ let a = {i}; }}"))
            .collect::<Vec<_>>()
            .join("\n");
        for c in chunk_code(&src) {
            assert!(!c.content.starts_with('…'), "code must be verbatim");
        }
    }

    #[test]
    fn a_chunk_names_the_definition_it_opens_with() {
        let src = format!(
            "use std::fmt;\n\npub fn alpha() {{\n{}\n}}\n\npub fn beta() {{\n    ok();\n}}\n",
            (0..80).map(|i| format!("    step({i});")).collect::<Vec<_>>().join("\n")
        );
        let chunks = chunk_code(&src);
        assert!(chunks.len() >= 2);
        let names: Vec<_> = chunks.iter().filter_map(|c| c.symbol.clone()).collect();
        assert!(names.contains(&"beta".to_string()), "got {names:?}");
    }

    #[test]
    fn symbol_extraction_handles_the_real_language_mix() {
        for (line, want) in [
            ("pub async fn enqueue_task(id: Uuid) {", Some("enqueue_task")),
            ("export function RepoMapPanel({ projectId }) {", Some("RepoMapPanel")),
            ("export const api = {", Some("api")),
            ("class WorktreeManager:", Some("WorktreeManager")),
            ("def resolve(spec, root):", Some("resolve")),
            ("interface Task {", Some("Task")),
            ("impl Orchestrator {", Some("Orchestrator")),
            ("    // just a comment", None),
        ] {
            let got = if starts_definition(line) { symbol_of(line) } else { None };
            assert_eq!(got.as_deref(), want, "for {line:?}");
        }
    }

    #[test]
    fn a_deeply_indented_line_is_not_a_top_level_definition() {
        // Otherwise every closure and nested helper becomes a chunk boundary.
        assert!(!starts_definition("            let inner = || 1;"));
        assert!(starts_definition("    pub fn method(&self) {"));
    }
}
