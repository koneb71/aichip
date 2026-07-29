//! Everything derived from a page body, computed once on write.
//!
//! Four things need to know what a page says: the search index, the revision
//! diff, the "what links here" list, and the text an agent is handed. Deriving
//! them in four places is how they drift — one indexes markup, another sees a
//! link the sanitiser removed, a third diffs HTML and reports every line
//! changed. So there is exactly one pass, and it runs at write time.
//!
//! Order matters and is not negotiable: **sanitise first, then project.**
//! Every extractor below is a string scanner, and a string scanner over
//! attacker-controlled HTML is only safe because ammonia has already
//! normalised it into balanced, quoted, well-formed markup.

use uuid::Uuid;

/// Everything a body write needs to store.
pub struct Prepared {
    /// The sanitised body — the only version that is ever stored.
    pub html: String,
    /// Block-per-line plain text. Feeds the search vector, the diff, and the
    /// text agents read.
    pub text: String,
    /// Pages this body links to, for backlinks.
    pub link_ids: Vec<Uuid>,
    /// Assets this body still references.
    pub asset_ids: Vec<Uuid>,
}

/// `to_tsvector` refuses inputs over 1 MB, and the search column is *generated*
/// — so it is computed on INSERT. One oversized page would make every future
/// write to that row fail with an error nobody would recognise. Capped here,
/// before the value is ever bound to a statement.
pub const MAX_TEXT_CHARS: usize = 500_000;

pub fn prepare(raw_html: &str) -> Prepared {
    let html = super::sanitize::article_html(raw_html);
    Prepared {
        text: to_text(&html),
        link_ids: extract_ids(&html, "href=\"/knowledge/"),
        asset_ids: extract_ids(&html, "src=\"/api/kb/assets/"),
        html,
    }
}

/// Tags that end a line of text.
///
/// The diff is line-based, so this list is what decides whether a reviewer sees
/// "one paragraph changed" or "the entire document changed". Without a newline
/// at every block boundary the whole page collapses to a single line and the
/// diff is worthless.
const BLOCK_ENDS: &[&str] = &[
    "</p>", "</h1>", "</h2>", "</h3>", "</h4>", "</h5>", "</h6>", "</li>", "</tr>", "</pre>",
    "</blockquote>", "</div>", "</figcaption>", "</summary>", "<br>", "<br/>", "<br />",
];

/// Project sanitised HTML down to block-per-line plain text.
pub fn to_text(html: &str) -> String {
    let mut marked = String::with_capacity(html.len() + 64);
    let mut rest = html;
    'outer: while !rest.is_empty() {
        for tag in BLOCK_ENDS {
            if rest.starts_with(tag) {
                marked.push('\n');
                rest = &rest[tag.len()..];
                continue 'outer;
            }
        }
        let ch = rest.chars().next().expect("non-empty");
        // A space before every remaining tag, so `</b><b>` doesn't weld two
        // words together into one that matches neither.
        if ch == '<' {
            marked.push(' ');
        }
        marked.push(ch);
        rest = &rest[ch.len_utf8()..];
    }

    let stripped = ammonia::Builder::empty().clean(&marked).to_string();
    let decoded = decode_entities(&stripped);

    let mut out = String::with_capacity(decoded.len());
    for line in decoded.lines() {
        // Collapse whitespace *within* a block only — collapsing across blocks
        // is what would destroy the line structure this exists to create.
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if collapsed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&collapsed);
        if out.chars().count() >= MAX_TEXT_CHARS {
            break;
        }
    }
    if out.chars().count() > MAX_TEXT_CHARS {
        out = out.chars().take(MAX_TEXT_CHARS).collect();
    }
    out
}

/// Pull UUIDs out of every occurrence of `prefix` followed by an id.
fn extract_ids(html: &str, prefix: &str) -> Vec<Uuid> {
    let mut ids = Vec::new();
    // The prefix carries its own `attribute="` opener, so prose that merely
    // mentions the path cannot forge a link — the scan only matches where the
    // sanitiser actually produced an attribute.
    for part in html.split(prefix).skip(1) {
        let raw: String = part.chars().take_while(|c| *c != '"' && *c != '/').collect();
        if let Ok(id) = Uuid::parse_str(&raw) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}

/// Ammonia escapes the text it strips tags from; this is plain text, so the
/// entities have to come back out.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_block_becomes_its_own_line() {
        // The diff is line-based; one line means one unreadable diff.
        let text = to_text("<h2>Title</h2><p>First.</p><p>Second.</p>");
        assert_eq!(text, "Title\nFirst.\nSecond.");
    }

    #[test]
    fn list_items_are_separate_lines() {
        assert_eq!(to_text("<ul><li>a</li><li>b</li></ul>"), "a\nb");
    }

    /// A nested block used to emit an empty outer line, which shows up in the
    /// diff as a phantom changed row.
    #[test]
    fn nesting_does_not_produce_empty_lines() {
        let text = to_text("<ul><li><p>only</p></li></ul>");
        assert_eq!(text, "only");
        assert!(!text.contains("\n\n"));
    }

    #[test]
    fn table_cells_stay_on_their_row() {
        assert_eq!(
            to_text("<table><tr><td>a</td><td>b</td></tr><tr><td>c</td></tr></table>"),
            "a b\nc"
        );
    }

    #[test]
    fn preformatted_text_keeps_its_words() {
        let text = to_text("<pre><code>make deploy --now</code></pre>");
        assert_eq!(text, "make deploy --now");
    }

    /// The scanner must not mistake escaped markup inside a code block for a
    /// real tag. Sanitising first is what makes that true.
    #[test]
    fn escaped_markup_inside_code_is_text_not_structure() {
        let p = prepare("<pre><code>&lt;/p&gt; is just text</code></pre>");
        assert_eq!(p.text, "</p> is just text");
    }

    #[test]
    fn adjacent_inline_tags_do_not_weld_words_together() {
        // "boldtext" would match neither word in a search.
        assert_eq!(to_text("<p><b>bold</b><i>text</i></p>"), "bold text");
    }

    /// Cleaning a projection again must not change it, or an edit-save-edit
    /// cycle slowly rewrites a document nobody touched.
    #[test]
    fn projecting_is_idempotent_over_its_own_output() {
        let once = to_text("<h2>A</h2><p>b &amp; c</p>");
        assert_eq!(to_text(&once), once);
    }

    #[test]
    fn page_links_are_collected_for_backlinks() {
        let id = Uuid::new_v4();
        let p = prepare(&format!(r#"<p>see <a href="/knowledge/{id}">that</a></p>"#));
        assert_eq!(p.link_ids, vec![id]);
    }

    #[test]
    fn the_same_link_twice_is_one_backlink() {
        let id = Uuid::new_v4();
        let p = prepare(&format!(
            r#"<a href="/knowledge/{id}">a</a><a href="/knowledge/{id}">b</a>"#
        ));
        assert_eq!(p.link_ids.len(), 1);
    }

    /// A backlink list citing a link the sanitiser removed would point at
    /// content no reader can see. Extracting from the *cleaned* html is what
    /// makes that impossible.
    #[test]
    fn a_link_the_sanitiser_removed_is_never_recorded() {
        let id = Uuid::new_v4();
        let p = prepare(&format!(
            r#"<a href="javascript:alert(1)">x</a><a href="/knowledge/{id}" onclick="evil()">y</a>"#
        ));
        assert_eq!(p.link_ids, vec![id]);
        assert!(!p.html.contains("javascript:"));
        assert!(!p.html.contains("onclick"));
    }

    #[test]
    fn referenced_assets_are_collected() {
        let id = Uuid::new_v4();
        let p = prepare(&format!(r#"<img src="/api/kb/assets/{id}" alt="x">"#));
        assert_eq!(p.asset_ids, vec![id]);
    }

    /// A page that quotes a URL in its prose is describing a link, not making
    /// one — a backlink graph built from prose would be full of edges nobody
    /// created.
    #[test]
    fn prose_that_mentions_a_page_path_does_not_forge_a_link() {
        let id = Uuid::new_v4();
        let p = prepare(&format!("<p>the page lives at /knowledge/{id} on this host</p>"));
        assert!(p.link_ids.is_empty(), "{:?}", p.link_ids);
    }

    #[test]
    fn a_malformed_id_is_ignored_rather_than_panicking() {
        let p = prepare(r#"<a href="/knowledge/not-a-uuid">x</a>"#);
        assert!(p.link_ids.is_empty());
    }

    /// A generated search column is computed on INSERT, and `to_tsvector`
    /// refuses inputs over 1 MB — so an uncapped body would make every later
    /// write to that row fail.
    #[test]
    fn the_projection_is_capped() {
        let huge = format!("<p>{}</p>", "word ".repeat(400_000));
        assert!(to_text(&huge).chars().count() <= MAX_TEXT_CHARS);
    }

    #[test]
    fn prepare_stores_only_sanitised_html() {
        let p = prepare("<p>ok</p><script>alert(1)</script>");
        assert!(p.html.contains("ok"));
        assert!(!p.html.contains("script"));
    }
}
