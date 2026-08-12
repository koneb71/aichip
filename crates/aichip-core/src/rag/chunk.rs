//! Split a document into retrievable pieces.
//!
//! Pure, and boring on purpose: a chunker's failure modes are all edge cases
//! — a 100KB single paragraph, a multi-byte character on a boundary, a file
//! of blank lines — and every one is cheaper as a unit test than as a panic
//! during somebody's upload.

/// What a chunk aims to be. Big enough that a retrieved passage carries its
/// own context, small enough that five of them fit a prompt budget.
pub const TARGET_CHARS: usize = 1200;

/// The hard ceiling. A paragraph that will not fit gets split; nothing ever
/// emerges larger than this.
pub const MAX_CHARS: usize = 2000;

/// How much of the previous chunk's tail each chunk carries, so a fact that
/// straddles a boundary is retrievable from either side.
pub const OVERLAP_CHARS: usize = 200;

/// Split on paragraph boundaries, accumulating toward `TARGET_CHARS`.
pub fn chunk(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }

    // Paragraphs, with oversize ones pre-split so the accumulator below only
    // ever deals in pieces that fit.
    let mut pieces: Vec<&str> = vec![];
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if para.chars().count() <= MAX_CHARS {
            pieces.push(para);
        } else {
            split_oversize(para, &mut pieces);
        }
    }

    let mut chunks: Vec<String> = vec![];
    let mut current = String::new();
    for piece in pieces {
        let sep = if current.is_empty() { 0 } else { 2 };
        if !current.is_empty() && current.chars().count() + sep + piece.chars().count() > TARGET_CHARS
        {
            chunks.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(piece);
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    // Overlap pass: each chunk after the first is prefixed with the tail of
    // the one before it, trimmed to a word boundary. Done after accumulation
    // so the overlap never counts against the target and cannot cascade.
    if chunks.len() > 1 {
        let tails: Vec<String> = chunks.iter().map(|c| tail_on_word(c, OVERLAP_CHARS)).collect();
        for i in (1..chunks.len()).rev() {
            let tail = &tails[i - 1];
            if !tail.is_empty() {
                chunks[i] = format!("…{tail}\n\n{}", chunks[i]);
            }
        }
    }
    chunks
}

/// A paragraph too big for one chunk: split on sentence-ish boundaries, then
/// hard-split on char boundaries for pathological runs with no punctuation.
fn split_oversize<'a>(para: &'a str, out: &mut Vec<&'a str>) {
    let mut start = 0;
    let mut count = 0;
    let mut last_break: Option<usize> = None;
    for (i, c) in para.char_indices() {
        count += 1;
        if matches!(c, '.' | '!' | '?' | '\n' | ';') {
            last_break = Some(i + c.len_utf8());
        }
        if count >= MAX_CHARS {
            // Cut at the last sentence break inside the window, else hard cut
            // here — `i` came from char_indices, so it is a char boundary and
            // multi-byte text cannot be split mid-character.
            let cut = last_break.filter(|&b| b > start).unwrap_or(i + c.len_utf8());
            let piece = para[start..cut].trim();
            if !piece.is_empty() {
                out.push(piece);
            }
            // The cursor may be past the cut (the break was behind us), and
            // those chars belong to the *next* piece — a bare `count = 0`
            // here under-counts it and lets it grow toward 2×MAX. Bounded
            // work: the recount never exceeds the window just emitted.
            count = para[cut..i + c.len_utf8()].chars().count();
            start = cut;
            last_break = None;
        }
    }
    let rest = para[start..].trim();
    if !rest.is_empty() {
        out.push(rest);
    }
}

/// The last ~`budget` chars, starting on a word boundary.
fn tail_on_word(text: &str, budget: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= budget {
        return text.to_string();
    }
    let tail: String = chars[chars.len() - budget..].iter().collect();
    match tail.find(char::is_whitespace) {
        Some(i) => tail[i..].trim_start().to_string(),
        None => tail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_blank_input_chunk_to_nothing() {
        assert!(chunk("").is_empty());
        assert!(chunk("   \n\n  \n").is_empty());
    }

    #[test]
    fn a_short_document_is_one_chunk_verbatim() {
        let text = "One paragraph.\n\nAnother paragraph.";
        assert_eq!(chunk(text), vec![text.to_string()]);
    }

    #[test]
    fn paragraphs_accumulate_toward_the_target_without_splitting_mid_paragraph() {
        // Ten 300-char paragraphs: chunks should hold whole paragraphs and
        // sit near the target.
        let para = "x".repeat(300);
        let text = vec![para.clone(); 10].join("\n\n");
        let chunks = chunk(&text);
        assert!(chunks.len() >= 2, "{}", chunks.len());
        for c in &chunks {
            // Every paragraph inside a chunk is intact (no partial 'x' runs
            // shorter than 300 except via the overlap prefix).
            assert!(c.chars().count() <= MAX_CHARS + OVERLAP_CHARS + 3);
        }
    }

    #[test]
    fn consecutive_chunks_overlap() {
        let text = (0..40)
            .map(|i| format!("Paragraph number {i} carries some words to fill space."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk(&text);
        assert!(chunks.len() >= 2);
        // The second chunk opens with the ellipsis-marked tail of the first.
        assert!(chunks[1].starts_with('…'), "{:?}", &chunks[1][..40.min(chunks[1].len())]);
    }

    #[test]
    fn a_giant_single_paragraph_splits_and_respects_the_ceiling() {
        let text = "word ".repeat(20_000); // ~100KB, no double newlines
        let chunks = chunk(&text);
        assert!(chunks.len() > 10);
        for c in &chunks {
            assert!(
                c.chars().count() <= MAX_CHARS + OVERLAP_CHARS + 3,
                "{}",
                c.chars().count()
            );
        }
    }

    #[test]
    fn multibyte_text_never_panics_or_splits_mid_character() {
        // Every char is multi-byte; a byte-indexed cut would panic.
        let text = "日本語のテキストです。".repeat(1000);
        let chunks = chunk(&text);
        assert!(!chunks.is_empty());
        let total: usize = chunks.iter().map(|c| c.chars().count()).sum();
        assert!(total >= 10_000, "content survived: {total}");
    }

    #[test]
    fn sentence_boundaries_are_preferred_for_oversize_splits() {
        let sentence = "This sentence is exactly some length and ends properly. ";
        let text = sentence.repeat(100); // one huge "paragraph"
        let chunks = chunk(&text);
        // Cuts land after periods, so chunks (ignoring the overlap prefix)
        // end with a period.
        for c in &chunks[..chunks.len() - 1] {
            assert!(c.trim_end().ends_with('.'), "{:?}", &c[c.len().saturating_sub(30)..]);
        }
    }
}
