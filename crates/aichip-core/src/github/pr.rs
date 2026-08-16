//! Finishing a card as a pull request.
//!
//! The same relationship with `gh` as everything else here: run the binary the
//! user is already logged into, read its stdout, hold no credential. aichip
//! never talks to api.github.com itself, so there is nothing here that could
//! act as anyone but the account `gh auth status` reports.
//!
//! ## Why `gh pr view` and not `gh pr checks`
//!
//! `gh pr checks` exits **8** when checks are still running. A runner that
//! treats a non-zero exit as failure would report an error on the single most
//! common state a pull request is in. `gh pr view --json statusCheckRollup`
//! carries the same information and exits 0.

use super::{gh, GhError};
use serde::Deserialize;
use std::path::Path;

/// Where a pull request has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Open,
    /// Open, but explicitly not asking to be merged yet.
    Draft,
    Merged,
    Closed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Draft => "draft",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "open" => Self::Open,
            "draft" => Self::Draft,
            "merged" => Self::Merged,
            "closed" => Self::Closed,
            _ => return None,
        })
    }
}

/// Every check on the head commit, as one answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Checks {
    /// No checks are configured. Deliberately not `Passing` — a repository
    /// that runs nothing has proved nothing.
    None,
    Pending,
    Passing,
    Failing,
}

impl Checks {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pending => "pending",
            Self::Passing => "passing",
            Self::Failing => "failing",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "none" => Self::None,
            "pending" => Self::Pending,
            "passing" => Self::Passing,
            "failing" => Self::Failing,
            _ => return None,
        })
    }
}

/// What the reviewers have said, when the repository asks anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

impl Review {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes_requested",
            Self::ReviewRequired => "review_required",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "approved" | "APPROVED" => Self::Approved,
            "changes_requested" | "CHANGES_REQUESTED" => Self::ChangesRequested,
            "review_required" | "REVIEW_REQUIRED" => Self::ReviewRequired,
            // `""` is what GitHub sends for a repository with no review rules,
            // which is not the same as "nobody has approved it yet".
            _ => return None,
        })
    }
}

/// A pull request, as much of it as aichip keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: i32,
    pub url: String,
    pub state: State,
    pub checks: Checks,
    pub review: Option<Review>,
}

// ── What `gh pr view --json` sends ──────────────────────────────────────────

/// `gh --json` answers in camelCase — `isDraft`, `reviewDecision`,
/// `statusCheckRollup`. Without this every draft read as open and every review
/// decision silently defaulted to none.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewJson {
    #[serde(default)]
    number: i32,
    #[serde(default)]
    url: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    review_decision: String,
    #[serde(default)]
    status_check_rollup: Vec<CheckEntry>,
}

/// One entry of `statusCheckRollup`, which is **two shapes in one list**.
///
/// A CheckRun (GitHub Actions and friends) reports `status` plus `conclusion`;
/// a StatusContext (the older commit-status API, still what many bots use)
/// reports `state` and neither of the others. Verified against cli/cli, whose
/// entries carry `status`/`conclusion`. All three are optional so an entry of
/// either shape parses, and [`entry_state`] decides which field is the answer.
#[derive(Deserialize, Debug, Clone, Default)]
pub struct CheckEntry {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub conclusion: String,
    #[serde(default)]
    pub state: String,
}

impl CheckEntry {
    /// The one field that says how this check ended, whichever shape it is.
    ///
    /// `conclusion` first: a finished CheckRun has `status: "COMPLETED"`, which
    /// says it stopped and nothing about whether it passed.
    fn outcome(&self) -> &str {
        for candidate in [&self.conclusion, &self.status, &self.state] {
            if !candidate.trim().is_empty() {
                return candidate.trim();
            }
        }
        ""
    }
}

/// Every check as one answer, decided fail-closed.
///
/// The rule that matters: a state this does not recognise counts as `Pending`,
/// never `Passing`. It is the sibling of `an_unknown_state_is_not_treated_as_working`
/// next door — the day GitHub adds a check outcome, a card must not read green
/// because of it.
pub fn roll_up_checks(entries: &[CheckEntry]) -> Checks {
    if entries.is_empty() {
        return Checks::None;
    }
    let mut pending = false;
    for entry in entries {
        match entry.outcome().to_ascii_uppercase().as_str() {
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED"
            | "STARTUP_FAILURE" => {
                // One failure is the answer; nothing later can improve it.
                return Checks::Failing;
            }
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => {}
            // PENDING, QUEUED, IN_PROGRESS, EXPECTED, WAITING, REQUESTED — and
            // anything at all that is new.
            _ => pending = true,
        }
    }
    if pending {
        Checks::Pending
    } else {
        Checks::Passing
    }
}

/// Read `gh pr view --json …` output.
pub fn parse_pr_view(json: &str) -> Result<PullRequest, String> {
    let raw: ViewJson = serde_json::from_str(json)
        .map_err(|e| format!("could not read what gh reported about this pull request: {e}"))?;
    if raw.number == 0 {
        return Err("gh reported a pull request with no number".into());
    }
    let state = match raw.state.to_ascii_uppercase().as_str() {
        "MERGED" => State::Merged,
        "CLOSED" => State::Closed,
        _ if raw.is_draft => State::Draft,
        _ => State::Open,
    };
    Ok(PullRequest {
        number: raw.number,
        url: raw.url,
        state,
        checks: roll_up_checks(&raw.status_check_rollup),
        review: Review::parse(&raw.review_decision),
    })
}

/// The fields every read asks for. One list, so the stored row and the live
/// refresh can never disagree about what was fetched.
const VIEW_FIELDS: &str = "number,url,state,isDraft,mergedAt,reviewDecision,statusCheckRollup";

/// Look a pull request up by number, or by the branch it comes from.
///
/// `GhError::NoPullRequest` is the ordinary answer for a branch that has none
/// — it is how the caller learns to create one, not a failure.
pub async fn view(cwd: &Path, selector: &str) -> Result<PullRequest, GhError> {
    let out = gh(Some(cwd), &["pr", "view", selector, "--json", VIEW_FIELDS]).await?;
    parse_pr_view(&out).map_err(GhError::Failed)
}

/// Open one.
///
/// `--head` is not decoration. Without it, `gh` prompts about where to push a
/// branch it thinks is unpushed — and this process has no console, so the
/// prompt is not a slow answer but one that never returns.
///
/// `--body` rather than `--body-file -`, because stdin is closed for that same
/// reason.
pub async fn create(
    cwd: &Path,
    base: &str,
    head: &str,
    title: &str,
    body: &str,
) -> Result<(), GhError> {
    gh(
        Some(cwd),
        &[
            "pr", "create", "--base", base, "--head", head, "--title", title, "--body", body,
        ],
    )
    .await
    .map(|_| ())
}

/// How long a body may be before it is cut.
///
/// GitHub's own limit is around 65 536 characters; stopping short of it means
/// the truncation is ours, visible, and says so — rather than GitHub's, silent,
/// and discovered by a reviewer.
const MAX_BODY: usize = 60_000;

/// What the pull request says, which is what the person asked for.
///
/// The task's own prompt, verbatim. It is the highest-fidelity description
/// available and it costs nothing. A model-written summary was the alternative
/// and is worse twice over: it spends a paid run per pull request, and this
/// repository's rules forbid attributing prose to an agent — so it would have
/// to be both expensive and undisclosed.
///
/// Nothing here announces what wrote the code. That is not an oversight, it is
/// the same rule the commit messages follow, and the test below is what keeps
/// it true when somebody later reaches for a footer.
/// Which issue this pull request should close, if any.
///
/// `Some` only when the card came from a GitHub issue **on the repository the
/// pull request is being opened against**. GitHub's cross-repository form
/// (`owner/repo#42`) only closes when the author has write access there, so a
/// card whose issue lives elsewhere gets nothing rather than a keyword that
/// silently does not work.
pub fn closes_number(
    source: Option<&str>,
    source_ref: Option<&str>,
    source_number: Option<i32>,
    project_repo: Option<&str>,
) -> Option<i32> {
    if source? != "github_issue" {
        return None;
    }
    let number = source_number?;
    // A half-written row — a source with no number — must not become
    // `Closes #` with nothing after it.
    if number <= 0 {
        return None;
    }
    let (issue_repo, _) = source_ref?.rsplit_once('#')?;
    (issue_repo == project_repo?).then_some(number)
}

pub fn pr_body(prompt: &str, branch: &str, closes: Option<i32>) -> String {
    let prompt = prompt.trim();
    let mut body = String::from("### What this card asked for\n\n");
    if prompt.is_empty() {
        body.push_str("_This card had no description._\n");
    } else if prompt.len() > MAX_BODY {
        // On a character boundary, or the string is not valid UTF-8 any more.
        let mut cut = MAX_BODY;
        while cut > 0 && !prompt.is_char_boundary(cut) {
            cut -= 1;
        }
        body.push_str(&prompt[..cut]);
        body.push_str("\n\n_… the rest of this card's description was too long to include._\n");
    } else {
        body.push_str(prompt);
        body.push('\n');
    }
    body.push_str(&format!("\nBranch: `{branch}`\n"));
    // Its own paragraph, and after the description rather than before it.
    // GitHub does not link the keyword inside a code fence, and a card whose
    // description ends in an unclosed fence would swallow a line placed above.
    if let Some(number) = closes {
        body.push_str(&format!("\nCloses #{number}\n"));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(status: &str, conclusion: &str) -> CheckEntry {
        CheckEntry {
            status: status.into(),
            conclusion: conclusion.into(),
            state: String::new(),
        }
    }

    #[test]
    fn no_checks_configured_is_not_the_same_as_passing() {
        // A repository that runs nothing has proved nothing, and a green tick
        // there would be the most confident wrong thing on the card.
        assert_eq!(roll_up_checks(&[]), Checks::None);
    }

    #[test]
    fn one_failure_decides_it_however_many_succeeded() {
        let entries = vec![
            entry("COMPLETED", "SUCCESS"),
            entry("COMPLETED", "FAILURE"),
            entry("COMPLETED", "SUCCESS"),
        ];
        assert_eq!(roll_up_checks(&entries), Checks::Failing);
    }

    #[test]
    fn anything_still_running_holds_the_whole_answer_back() {
        let entries = vec![entry("COMPLETED", "SUCCESS"), entry("IN_PROGRESS", "")];
        assert_eq!(roll_up_checks(&entries), Checks::Pending);
    }

    #[test]
    fn skipped_and_neutral_do_not_stop_it_passing() {
        let entries = vec![
            entry("COMPLETED", "SUCCESS"),
            entry("COMPLETED", "SKIPPED"),
            entry("COMPLETED", "NEUTRAL"),
        ];
        assert_eq!(roll_up_checks(&entries), Checks::Passing);
    }

    /// The rollup carries two different shapes in one list.
    #[test]
    fn a_status_context_is_counted_beside_a_check_run() {
        // CheckRun: status + conclusion. StatusContext: state, and nothing else.
        let mixed = vec![
            entry("COMPLETED", "SUCCESS"),
            CheckEntry {
                state: "FAILURE".into(),
                ..Default::default()
            },
        ];
        assert_eq!(roll_up_checks(&mixed), Checks::Failing);

        // And a finished CheckRun is read by its conclusion, not its status —
        // `COMPLETED` says it stopped, not that it passed.
        assert_eq!(
            roll_up_checks(&[entry("COMPLETED", "FAILURE")]),
            Checks::Failing
        );
    }

    /// Never delete this. The day GitHub adds a check outcome, a card must not
    /// read green because nothing here recognised it.
    #[test]
    fn a_state_nobody_has_seen_before_is_never_green() {
        let entries = vec![entry("COMPLETED", "SUCCESS"), entry("SOMETHING_NEW", "")];
        assert_eq!(roll_up_checks(&entries), Checks::Pending);

        let only_new = vec![CheckEntry {
            state: "MYSTERY".into(),
            ..Default::default()
        }];
        assert_eq!(roll_up_checks(&only_new), Checks::Pending);
    }

    /// Real output, captured from `gh pr view 1 --repo cli/cli` on gh 2.96.0.
    #[test]
    fn a_merged_pull_request_reads_as_merged() {
        let json = r#"{"isDraft":false,"mergedAt":"2019-10-04T16:01:04Z","number":1,
            "reviewDecision":"","state":"MERGED","statusCheckRollup":[],
            "url":"https://github.com/cli/cli/pull/1"}"#;
        let pr = parse_pr_view(json).unwrap();
        assert_eq!(pr.number, 1);
        assert_eq!(pr.state, State::Merged);
        assert_eq!(pr.url, "https://github.com/cli/cli/pull/1");
        assert_eq!(pr.checks, Checks::None);
        // `""` is what a repository with no review rules sends. Reading it as
        // "changes requested" or "waiting" would invent a blocker.
        assert_eq!(pr.review, None);
    }

    #[test]
    fn a_draft_is_open_but_not_asking_to_be_merged() {
        let json = r#"{"number":7,"url":"u","state":"OPEN","isDraft":true,
            "reviewDecision":"REVIEW_REQUIRED","statusCheckRollup":[]}"#;
        let pr = parse_pr_view(json).unwrap();
        assert_eq!(pr.state, State::Draft);
        assert_eq!(pr.review, Some(Review::ReviewRequired));

        let json = r#"{"number":7,"url":"u","state":"OPEN","isDraft":false,
            "reviewDecision":"APPROVED","statusCheckRollup":[]}"#;
        assert_eq!(parse_pr_view(json).unwrap().state, State::Open);
        assert_eq!(parse_pr_view(json).unwrap().review, Some(Review::Approved));
    }

    #[test]
    fn nonsense_is_an_error_rather_than_a_pull_request_that_looks_fine() {
        assert!(parse_pr_view("not json").is_err());
        // No number means gh answered about nothing; defaulting it to 0 and
        // storing that would make the card unaddressable forever.
        assert!(parse_pr_view(r#"{"url":"u","state":"OPEN"}"#).is_err());
    }

    /// CLAUDE.md: commits and pull request bodies carry no AI attribution of
    /// any kind. This test is what keeps that true when somebody later reaches
    /// for a footer.
    #[test]
    fn the_body_never_says_what_wrote_the_code() {
        // Both arms, so the closing line cannot become the place an attribution
        // slips in unnoticed.
        for closes in [None, Some(42)] {
            let body = pr_body(
                "Add a retry button to the board",
                "aichip/retry-a1b2c3d4",
                closes,
            );
            assert!(body.contains("Add a retry button"));
            assert!(body.contains("aichip/retry-a1b2c3d4"));
            for banned in [
                "Co-Authored-By",
                "Generated with",
                "claude.ai",
                "claude.com",
                "Claude",
                "🤖",
                "AI-assisted",
            ] {
                assert!(
                    !body.contains(banned),
                    "the pull request body attributes itself ({closes:?}): {banned}"
                );
            }
        }
    }

    #[test]
    fn a_card_from_an_issue_closes_it_and_one_typed_by_hand_does_not() {
        let with = pr_body("do the thing", "b", Some(42));
        assert!(with.contains("\nCloses #42\n"), "{with}");
        // After the description: GitHub does not link the keyword inside a code
        // fence, and a prompt ending in an unclosed one would swallow it.
        assert!(with.find("do the thing").unwrap() < with.find("Closes #42").unwrap());

        // A card nobody imported gets byte-identical output to before.
        assert_eq!(
            pr_body("do the thing", "b", None),
            pr_body_before("do the thing", "b")
        );
    }

    /// What the body was before `closes` existed, so the no-issue case is
    /// pinned as unchanged rather than merely believed to be.
    fn pr_body_before(prompt: &str, branch: &str) -> String {
        format!(
            "### What this card asked for\n\n{}\n\nBranch: `{branch}`\n",
            prompt.trim()
        )
    }

    #[test]
    fn an_overlong_description_still_carries_its_closing_line() {
        // The naive implementation truncates and returns, losing it.
        let body = pr_body(&"x".repeat(MAX_BODY + 5_000), "b", Some(7));
        assert!(body.contains("too long to include"));
        assert!(
            body.contains("Closes #7"),
            "truncation ate the closing line"
        );
    }

    #[test]
    fn only_an_issue_on_this_repository_is_closed() {
        // The ordinary case.
        assert_eq!(
            closes_number(
                Some("github_issue"),
                Some("cli/cli#42"),
                Some(42),
                Some("cli/cli")
            ),
            Some(42)
        );
        // A card somebody typed.
        assert_eq!(closes_number(None, None, None, Some("cli/cli")), None);
        // An issue from another repository: the cross-repo form only closes
        // with write access there, so promising it would be a lie.
        assert_eq!(
            closes_number(
                Some("github_issue"),
                Some("other/repo#42"),
                Some(42),
                Some("cli/cli")
            ),
            None
        );
        // A half-written row must not produce `Closes #`.
        assert_eq!(
            closes_number(
                Some("github_issue"),
                Some("cli/cli#42"),
                None,
                Some("cli/cli")
            ),
            None
        );
        assert_eq!(
            closes_number(
                Some("github_issue"),
                Some("cli/cli#0"),
                Some(0),
                Some("cli/cli")
            ),
            None
        );
        // A project whose repository is unknown cannot be compared against.
        assert_eq!(
            closes_number(Some("github_issue"), Some("cli/cli#42"), Some(42), None),
            None
        );
        // A future importer must not inherit GitHub's convention.
        assert_eq!(
            closes_number(
                Some("jira_ticket"),
                Some("cli/cli#42"),
                Some(42),
                Some("cli/cli")
            ),
            None
        );
    }

    #[test]
    fn an_overlong_description_is_cut_visibly_rather_than_by_github() {
        let long = "x".repeat(MAX_BODY + 5_000);
        let body = pr_body(&long, "b", None);
        assert!(body.len() < MAX_BODY + 500);
        assert!(
            body.contains("too long to include"),
            "a silent cut is a lie"
        );

        // A multi-byte character straddling the cut must not produce invalid
        // UTF-8 — the string is returned, so this would be a panic.
        let wide = "é".repeat(MAX_BODY);
        let body = pr_body(&wide, "b", None);
        assert!(body.contains("too long to include"));
    }

    #[test]
    fn a_card_with_no_description_still_gets_a_body() {
        // `gh pr create --body ""` is legal but leaves a reviewer nothing.
        let body = pr_body("   ", "b", None);
        assert!(body.contains("no description"));
        assert!(!body.trim().is_empty());
    }

    #[test]
    fn a_description_reaches_github_exactly_as_written() {
        // Everything is an argv element, never a shell string, so nothing here
        // needs escaping and nothing may be mangled on the way.
        let tricky = "run `$(rm -rf /)` and \"quote\" it\nplus a newline";
        assert!(pr_body(tricky, "b", None).contains(tricky));
    }

    #[test]
    fn every_stored_word_round_trips() {
        // These strings are written to the database and read back, so a rename
        // on one side and not the other would silently blank a card's chip.
        for s in [State::Open, State::Draft, State::Merged, State::Closed] {
            assert_eq!(State::parse(s.as_str()), Some(s));
        }
        for c in [
            Checks::None,
            Checks::Pending,
            Checks::Passing,
            Checks::Failing,
        ] {
            assert_eq!(Checks::parse(c.as_str()), Some(c));
        }
        for r in [
            Review::Approved,
            Review::ChangesRequested,
            Review::ReviewRequired,
        ] {
            assert_eq!(Review::parse(r.as_str()), Some(r));
        }
    }
}
