//! The part of the map that is worth putting in a prompt.
//!
//! An agent told "add a settings page" starts cold and burns its first tool
//! calls rediscovering where things live. The Map already knows: every file,
//! the symbols it defines, and how depended-on it is. The temptation is to
//! paste the lot.
//!
//! **It does not fit, and that measurement is why this is a slice.** The bare
//! list of this repository's code paths — no signatures, no symbols, just
//! paths — is over ten thousand characters. Anything calling itself "the repo
//! map" at a couple of thousand is a sample, and a sample chosen by
//! alphabetical accident is worse than nothing: it teaches the agent that the
//! map is unreliable, which is the one thing it must not think.
//!
//! So this names only what the *request* points at. Identifiers in what the
//! person asked are matched against symbol names and paths; the files that
//! match are listed with the symbols that matched, best first, until the
//! budget runs out — and what was dropped is admitted in-band rather than
//! trailing off.
//!
//! ## Ranked by relevance, not by importance
//!
//! PageRank over the import graph is already computed and is deliberately
//! *not* the ordering here. Measured on this repository the top-ranked files
//! are `api.ts`, `db.rs` and `Icon.tsx` — infrastructure hubs, and precisely
//! the files an agent least needs pointed out. Rank is the tiebreaker between
//! two files that matched equally well, which is the job it is actually good
//! at.
//!
//! ## It says which tree it read
//!
//! A board card runs in a git worktree on its own branch while the index
//! reconciles the project's checkout, so the map can legitimately describe a
//! different tree at a different commit. Every rendering names the branch and
//! short sha it came from, and the framing keeps the Brain's rule: where this
//! disagrees with the code in front of you, the code is right.

use crate::fence::{MAP_BEGIN as BEGIN, MAP_END as END};

/// How much of the map a prompt will carry.
///
/// Two thousand characters is about twenty files with their matching symbols
/// — enough to answer "where does this live" for a focused request, and small
/// enough that it never crowds out the request itself.
pub const BUDGET: usize = 2000;

/// How many symbols are listed for one file before the rest are counted.
const SYMBOLS_PER_FILE: usize = 4;

/// A symbol the index found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sym {
    pub name: String,
    pub kind: String,
    pub line: i32,
}

/// One indexed file and what it defines.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    /// PageRank over the import graph. Tiebreaker only — see the module doc.
    pub rank: f32,
    pub symbols: Vec<Sym>,
}

/// The map as loaded, before anything has been asked of it.
#[derive(Debug, Clone)]
pub struct RepoMap {
    /// The branch the index was read from.
    pub branch: String,
    /// Short sha, same.
    pub sha: String,
    pub files: Vec<FileEntry>,
}

/// The identifier-ish words in a request.
///
/// Split on anything that is not alphanumeric or underscore, then also split
/// camelCase and snake_case into their parts, because a person writing "fix
/// the vet_task check" and a symbol named `vetTask` should still meet. Short
/// and very common words are dropped: matching on "the" or "add" would make
/// every file relevant, which is the same as none of them being.
pub fn identifiers(request: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "add", "new", "fix", "use", "get", "set", "not", "but",
        "this", "that", "from", "into", "make", "let", "run", "all", "any", "out", "its", "our",
        "can", "has", "was", "are", "you", "when", "what", "why", "how", "page", "file", "code",
        "please", "should", "would", "could", "need", "want", "just", "also", "then", "than",
        "there", "here", "some", "more", "less", "very", "much", "does", "did", "will",
    ];
    let mut out: Vec<String> = Vec::new();
    let mut push = |w: &str| {
        let w = w.to_lowercase();
        if w.len() >= 3 && !STOP.contains(&w.as_str()) && !out.contains(&w) {
            out.push(w);
        }
    };
    for word in request.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if word.is_empty() {
            continue;
        }
        push(word);
        // camelCase and snake_case, split into parts. A request rarely spells
        // an identifier exactly as the code does.
        for part in word.split('_') {
            push(part);
        }
        let mut current = String::new();
        for ch in word.chars() {
            if ch.is_uppercase() && !current.is_empty() {
                push(&current);
                current.clear();
            }
            current.push(ch);
        }
        push(&current);
    }
    out
}

/// How well one file answers the request, and which symbols said so.
fn score(file: &FileEntry, wanted: &[String]) -> (usize, Vec<Sym>) {
    let mut hits: Vec<Sym> = Vec::new();
    let mut points = 0usize;
    for sym in &file.symbols {
        let lower = sym.name.to_lowercase();
        if wanted.iter().any(|w| lower.contains(w.as_str())) {
            points += 2; // A symbol match is the strong signal.
            hits.push(sym.clone());
        }
    }
    // A path match is weaker but real: "the settings page" should find
    // `settings.tsx` even when nothing inside it is called settings.
    let path = file.path.to_lowercase();
    if wanted.iter().any(|w| path.contains(w.as_str())) {
        points += 1;
    }
    (points, hits)
}

/// Pick the files worth naming, best first.
///
/// Deterministic: ties break on rank, then on path, so two runs of the same
/// request against the same index produce the same block. A map that
/// reshuffles between runs is one nobody can diff.
fn pick<'a>(map: &'a RepoMap, wanted: &[String]) -> Vec<(&'a FileEntry, Vec<Sym>)> {
    let mut scored: Vec<(usize, &FileEntry, Vec<Sym>)> = map
        .files
        .iter()
        .filter_map(|f| {
            let (points, hits) = score(f, wanted);
            (points > 0).then_some((points, f, hits))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.rank.total_cmp(&a.1.rank))
            .then(a.1.path.cmp(&b.1.path))
    });
    scored.into_iter().map(|(_, f, hits)| (f, hits)).collect()
}

/// Render the slice, or an empty string when nothing matched.
///
/// Empty is the common case for a request that names no code — "what did this
/// cost", "summarise the board" — and it has to be *exactly* empty so the
/// caller can fold it in unconditionally without changing those prompts.
pub fn render(map: &RepoMap, request: &str, budget: usize) -> String {
    let wanted = identifiers(request);
    if wanted.is_empty() {
        return String::new();
    }
    let picked = pick(map, &wanted);
    if picked.is_empty() {
        return String::new();
    }

    let mut body = String::new();
    let mut shown = 0usize;
    for (file, hits) in &picked {
        let mut line = format!("{}", file.path);
        if !hits.is_empty() {
            let names: Vec<String> = hits
                .iter()
                .take(SYMBOLS_PER_FILE)
                .map(|s| format!("{} {}:{}", s.kind, s.name, s.line))
                .collect();
            line.push_str(&format!(" — {}", names.join(", ")));
            if hits.len() > SYMBOLS_PER_FILE {
                line.push_str(&format!(" (+{} more)", hits.len() - SYMBOLS_PER_FILE));
            }
        }
        line.push('\n');
        // Budget is checked before appending, so the block never exceeds it
        // and never ends mid-path.
        if body.len() + line.len() > budget {
            break;
        }
        body.push_str(&line);
        shown += 1;
    }
    if shown == 0 {
        return String::new();
    }
    let dropped = picked.len() - shown;
    if dropped > 0 {
        body.push_str(&format!(
            "…and {dropped} more matching file{} not shown.\n",
            if dropped == 1 { "" } else { "s" }
        ));
    }
    body
}

/// Fold the slice into a prompt.
///
/// Appended after the request, like every other block: what was asked stays
/// first. Byte-identical to the input when there is nothing to say.
pub fn augment_prompt(prompt: &str, map: Option<&RepoMap>) -> String {
    augment_prompt_for(prompt, prompt, map)
}

/// Append to one string while selecting against another.
///
/// The two differ wherever the Brain and the Skill have already been folded
/// in: those are prose, and matching identifiers against them drags in files
/// by coincidence of vocabulary — a Brain that mentions "the settings page"
/// would attach settings files to a request about billing. The request is
/// what the slice is *for*, so the request is what it reads.
pub fn augment_prompt_for(prompt: &str, request: &str, map: Option<&RepoMap>) -> String {
    let Some(map) = map else {
        return prompt.to_string();
    };
    let body = render(map, request, BUDGET);
    if body.is_empty() {
        return prompt.to_string();
    }
    // Paths and symbol names come from files in the repository, so a file
    // literally named after another family's opener would otherwise smuggle
    // one in. Same scrub every quoting feature applies.
    let body = neutralise(&body);
    format!(
        "{prompt}\n\n---\n\nSome files from this project that look related to the request, from \
the index of {} at {}. It is a pointer, not a specification — open what it names, and where it \
disagrees with the code in front of you, the code is right.\n\n{BEGIN}\n{body}{END}\n",
        map.branch, map.sha
    )
}

/// Strip every fence marker, including this module's own.
///
/// Its own too, and that is the difference from the other quoting features: a
/// Brain body is prose somebody typed, while this is generated from paths on
/// disk, so the only way a marker appears is that a file is named after one.
/// There is no reason to preserve it and every reason not to.
fn neutralise(text: &str) -> String {
    crate::fence::scrub_foreign(text, &[])
        .replace(BEGIN, "")
        .replace(END, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str, kind: &str, line: i32) -> Sym {
        Sym {
            name: name.into(),
            kind: kind.into(),
            line,
        }
    }

    fn map() -> RepoMap {
        RepoMap {
            branch: "main".into(),
            sha: "abc1234".into(),
            files: vec![
                FileEntry {
                    path: "crates/aichip-server/src/routes/tasks.rs".into(),
                    rank: 0.2,
                    symbols: vec![
                        sym("vet_task", "function", 378),
                        sym("move_task", "function", 41),
                    ],
                },
                FileEntry {
                    path: "web/src/lib/api.ts".into(),
                    // The measured hub: highest rank, and almost never what
                    // somebody is asking about.
                    rank: 9.9,
                    symbols: vec![sym("api", "const", 1483)],
                },
                FileEntry {
                    path: "web/src/pages/SettingsPage.tsx".into(),
                    rank: 0.1,
                    symbols: vec![sym("SettingsPage", "component", 12)],
                },
            ],
        }
    }

    #[test]
    fn finds_the_file_by_a_symbol_the_request_names() {
        let out = render(
            &map(),
            "where do we decide whether a card can start, near vet_task",
            BUDGET,
        );
        assert!(out.contains("routes/tasks.rs"));
        assert!(out.contains("function vet_task:378"));
        // The hub is not dragged in just for being important.
        assert!(
            !out.contains("api.ts"),
            "rank must not substitute for relevance:\n{out}"
        );
    }

    #[test]
    fn a_path_match_counts_when_no_symbol_does() {
        let out = render(&map(), "tidy up the settings page", BUDGET);
        assert!(out.contains("SettingsPage.tsx"));
    }

    #[test]
    fn splits_the_request_so_spelling_does_not_have_to_match() {
        // "vetTask" typed, `vet_task` defined.
        assert!(identifiers("check vetTask please").contains(&"vet".to_string()));
        // snake_case in the request, and the whole word too.
        let ids = identifiers("the move_task handler");
        assert!(ids.contains(&"move_task".to_string()));
        assert!(ids.contains(&"move".to_string()));
    }

    #[test]
    fn very_common_words_are_not_identifiers() {
        // Otherwise every file matches, which is the same as none of them.
        let ids = identifiers("please add the new file for this code");
        assert!(ids.is_empty(), "got {ids:?}");
    }

    #[test]
    fn the_stop_list_can_never_be_complete_and_that_is_survivable() {
        // A word nobody thought to list gets through — "refactor" here — and
        // the consequence is bounded: it matches no symbol and no path, so it
        // contributes nothing. The stop list is there to stop the *frequent*
        // words dragging in every file, not to be exhaustive.
        let ids = identifiers("refactor the widget");
        assert!(ids.contains(&"refactor".to_string()));
        assert_eq!(render(&map(), "refactor", BUDGET), "");
    }

    #[test]
    fn a_request_that_names_no_code_produces_nothing_at_all() {
        // Byte-identical, so this can be folded in unconditionally.
        let p = "what did this cost?";
        assert_eq!(augment_prompt(p, Some(&map())), p);
        assert_eq!(augment_prompt(p, None), p);
        // And a request full of identifiers that match nothing.
        assert_eq!(render(&map(), "quaternion holography", BUDGET), "");
    }

    #[test]
    fn the_budget_is_respected_and_what_was_dropped_is_admitted() {
        let mut m = map();
        m.files = (0..80)
            .map(|i| FileEntry {
                path: format!("crates/aichip-core/src/widget_{i:02}.rs"),
                rank: i as f32,
                symbols: vec![sym(&format!("widget_thing_{i:02}"), "function", i)],
            })
            .collect();
        let out = render(&m, "widget", 400);
        assert!(out.len() <= 400 + 60, "over budget: {} chars", out.len());
        assert!(out.contains("more matching files not shown"), "{out}");
        // Never cut mid-line.
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn selects_against_the_request_not_the_whole_prompt() {
        // The prompt by the time this runs already carries the Brain and the
        // Skill. A Brain that happens to mention settings must not attach
        // settings files to a request that never did.
        let prompt = "how much did we spend?\n\n<<<BEGIN PROJECT BRAIN>>>\nthe settings page is \
                      the fiddly one\n<<<END PROJECT BRAIN>>>";
        let out = augment_prompt_for(prompt, "how much did we spend?", Some(&map()));
        assert_eq!(
            out, prompt,
            "the brain's vocabulary leaked into the selection"
        );
    }

    #[test]
    fn is_deterministic() {
        let m = map();
        assert_eq!(
            render(&m, "vet_task and settings", BUDGET),
            render(&m, "vet_task and settings", BUDGET)
        );
    }

    #[test]
    fn names_the_tree_it_was_read_from() {
        // A card runs in a worktree on another branch; a map that does not say
        // which tree it describes is a map that can be quietly wrong.
        let out = augment_prompt("fix vet_task", Some(&map()));
        assert!(out.contains("main"));
        assert!(out.contains("abc1234"));
        assert!(out.contains("the code is right"));
    }

    #[test]
    fn a_file_named_after_a_fence_cannot_open_one() {
        // The repository is allowed to contain a file called anything at all.
        let mut m = map();
        m.files.push(FileEntry {
            path: format!("src/{}.rs", crate::fence::BRAIN_END),
            rank: 1.0,
            symbols: vec![sym("widget", "function", 1)],
        });
        m.files.push(FileEntry {
            path: format!("src/{BEGIN}.rs"),
            rank: 1.0,
            symbols: vec![sym("widget", "function", 2)],
        });
        let out = augment_prompt("widget", Some(&m));
        assert!(!out.contains(crate::fence::BRAIN_END));
        // Exactly one pair: its own opener and closer, and no forged extras.
        assert_eq!(out.matches(BEGIN).count(), 1, "{out}");
        assert_eq!(out.matches(END).count(), 1, "{out}");
    }
}

/// Load the map for a project, or `None` when there is nothing usable.
///
/// Best-effort by construction, like the Brain and the Skill beside it: a run
/// must never fail over context that is only ever additive. An unindexed
/// project, a failed index, a database hiccup — all `None`, and the prompt is
/// unchanged.
///
/// Symbols are capped per project rather than per file. A repository with
/// thirty thousand of them would otherwise pull its whole symbol table into
/// memory to answer one question, and the ones past the cap belong to the
/// lowest-ranked files, which is where the least useful answers live.
pub async fn for_run(db: &crate::db::Db, project_id: Option<uuid::Uuid>) -> Option<RepoMap> {
    use sqlx::Row;
    let project_id = project_id?;

    let head = sqlx::query(
        "SELECT i.head_sha, COALESCE(p.default_branch, 'main') AS branch
           FROM project_index i JOIN projects p ON p.id = i.project_id
          WHERE i.project_id = $1 AND i.phase NOT IN ('never', 'failed')",
    )
    .bind(project_id)
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten()?;

    let rows = sqlx::query(
        "SELECT d.rel_path, d.rank, s.name, s.kind, s.line
           FROM project_documents d
           LEFT JOIN project_symbols s ON s.document_id = d.id
          WHERE d.project_id = $1
          ORDER BY d.rank DESC, d.rel_path, s.line
          LIMIT 20000",
    )
    .bind(project_id)
    .fetch_all(&db.pool)
    .await
    .ok()?;

    let mut files: Vec<FileEntry> = Vec::new();
    for r in &rows {
        let path: String = r.get("rel_path");
        if files.last().map(|f| f.path != path).unwrap_or(true) {
            files.push(FileEntry {
                path,
                rank: r.get("rank"),
                symbols: Vec::new(),
            });
        }
        // The LEFT JOIN means a file with no symbols still arrives, with NULLs.
        if let Some(name) = r.get::<Option<String>, _>("name") {
            if let Some(f) = files.last_mut() {
                f.symbols.push(Sym {
                    name,
                    kind: r.get("kind"),
                    line: r.get("line"),
                });
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    let sha: Option<String> = head.get("head_sha");
    Some(RepoMap {
        branch: head.get("branch"),
        // Short, because the long one is noise in a prompt and nobody is
        // going to paste it anywhere.
        sha: sha
            .map(|s| s.chars().take(7).collect())
            .unwrap_or_else(|| "unknown".into()),
        files,
    })
}
