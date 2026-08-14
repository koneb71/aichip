//! Clarifying questions: the assistant asks instead of guessing.
//!
//! A request that reads as one thing and means another is the cheapest
//! mistake to prevent and one of the dearest to undo — the assistant creates
//! the wrong card, an agent starts on it, and the first sign is a diff nobody
//! wanted. Asking costs one click.
//!
//! **Options, not prose.** "Did you mean A or B?" written into a reply is a
//! question the person has to answer in a sentence, which the assistant then
//! has to parse. A closed set of options is unambiguous in both directions,
//! and it also forces the assistant to have *thought* of the alternatives
//! rather than merely noticing it was unsure.
//!
//! Everything here is pure: the validation is where a malformed question
//! turns into a broken card in the UI, so it is worth testing without a
//! database.

use serde::{Deserialize, Serialize};

/// One choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    /// What picking it means. Optional, but a label alone is often a word
    /// whose consequence only the assistant knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One question, with the options that answer it.
///
/// camelCase on the wire in both directions, and it has to be *both*: the
/// model sends what the tool schema advertises (`multiSelect`), and the
/// browser reads back what was stored. Without the rename the field arrives
/// under a name serde does not recognise, defaults to false, and a
/// multiple-choice question silently becomes a single-choice one — wrong in a
/// way nothing errors on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub question: String,
    /// A two-or-three word chip so several questions can be told apart at a
    /// glance without reading all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub options: Vec<Choice>,
    /// More than one answer allowed — for a genuine "which of these" rather
    /// than a fork in the road.
    #[serde(default)]
    pub multi_select: bool,
}

/// At most this many at once.
///
/// Four is already a lot to answer before any work starts. Past that the
/// assistant is interviewing rather than clarifying, and the honest move is to
/// ask the most important one and use the answer.
pub const MAX_QUESTIONS: usize = 4;
/// And at most this many options each — beyond four a list stops being a
/// choice and becomes a form.
pub const MAX_OPTIONS: usize = 4;
/// Two is the minimum that is actually a question. One option is a statement
/// with a button on it.
pub const MIN_OPTIONS: usize = 2;

const MAX_LABEL: usize = 60;
const MAX_TEXT: usize = 400;

/// Check and tidy what the assistant asked.
///
/// Returns the reason on refusal, phrased *at the model*: the string goes back
/// as the tool's error, and an error that says what to do instead is one the
/// assistant can act on without another round trip.
pub fn validate(mut questions: Vec<Question>) -> Result<Vec<Question>, String> {
    if questions.is_empty() {
        return Err("ask at least one question".into());
    }
    if questions.len() > MAX_QUESTIONS {
        return Err(format!(
            "at most {MAX_QUESTIONS} questions at once — ask the one that most \
             changes what you would do, and use the answer"
        ));
    }
    for q in &mut questions {
        q.question = trim_to(&q.question, MAX_TEXT);
        if q.question.is_empty() {
            return Err("every question needs text".into());
        }
        q.header = q
            .header
            .as_deref()
            .map(|h| trim_to(h, MAX_LABEL))
            .filter(|h| !h.is_empty());
        if q.options.len() < MIN_OPTIONS {
            return Err(format!(
                "\"{}\" needs at least {MIN_OPTIONS} options — one option is a \
                 statement, not a question. If there is only one way forward, \
                 say so and take it.",
                trim_to(&q.question, 60)
            ));
        }
        if q.options.len() > MAX_OPTIONS {
            return Err(format!(
                "at most {MAX_OPTIONS} options per question — the person can \
                 always type something else"
            ));
        }
        for o in &mut q.options {
            o.label = trim_to(&o.label, MAX_LABEL);
            if o.label.is_empty() {
                return Err("every option needs a label".into());
            }
            o.description = o
                .description
                .as_deref()
                .map(|d| trim_to(d, MAX_TEXT))
                .filter(|d| !d.is_empty());
        }
        // Two options spelled the same are one option and a bug — and the
        // answer would be ambiguous, since an answer is carried by its label.
        let mut seen = std::collections::HashSet::new();
        for o in &q.options {
            if !seen.insert(o.label.to_lowercase()) {
                return Err(format!("\"{}\" is offered twice", o.label));
            }
        }
    }
    Ok(questions)
}

/// Cut to a budget on a character boundary, never a byte one.
fn trim_to(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    t.chars().take(max).collect::<String>().trim_end().to_string()
}

/// The message the person's answer becomes.
///
/// A real user message rather than a resumed tool result, because that is what
/// it is: the thread has to read back as a conversation, and "Answered: Rust"
/// three turns later has to be findable by somebody scrolling.
pub fn answer_message(questions: &[Question], answers: &[Vec<String>]) -> String {
    let mut out = String::new();
    for (i, q) in questions.iter().enumerate() {
        let picked = answers.get(i).map(Vec::as_slice).unwrap_or(&[]);
        if picked.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        // The question is repeated, briefly. The assistant asked it and the
        // session remembers, but a bare "Rust" is unreadable to the person
        // scrolling back, and the thread is for them too.
        out.push_str(&format!(
            "{} → {}",
            q.header.clone().unwrap_or_else(|| trim_to(&q.question, 80)),
            picked.join(", ")
        ));
    }
    if out.is_empty() {
        "(no answer given — use your best judgement)".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, labels: &[&str]) -> Question {
        Question {
            question: text.into(),
            header: None,
            options: labels
                .iter()
                .map(|l| Choice { label: (*l).into(), description: None })
                .collect(),
            multi_select: false,
        }
    }

    #[test]
    fn a_well_formed_question_survives_unchanged() {
        let one = q("Which language?", &["Rust", "Go"]);
        assert_eq!(validate(vec![one.clone()]).unwrap(), vec![one]);
    }

    #[test]
    fn one_option_is_a_statement_and_is_refused() {
        // The failure this catches: an assistant that has already decided and
        // is asking for the form of consent rather than the substance.
        let e = validate(vec![q("Shall I?", &["Yes"])]).unwrap_err();
        assert!(e.contains("statement"), "{e}");
    }

    #[test]
    fn nothing_at_all_is_refused() {
        assert!(validate(vec![]).is_err());
        assert!(validate(vec![q("   ", &["a", "b"])]).is_err());
        assert!(validate(vec![q("Pick", &["", "b"])]).is_err());
    }

    #[test]
    fn too_many_questions_is_an_interview() {
        let many: Vec<_> = (0..5).map(|i| q(&format!("Q{i}?"), &["a", "b"])).collect();
        let e = validate(many).unwrap_err();
        assert!(e.contains("at most 4 questions"), "{e}");
    }

    #[test]
    fn too_many_options_is_a_form() {
        let e = validate(vec![q("Pick", &["a", "b", "c", "d", "e"])]).unwrap_err();
        assert!(e.contains("at most 4 options"), "{e}");
    }

    #[test]
    fn the_same_option_twice_is_refused_however_it_is_capitalised() {
        // An answer travels as its label, so two options that differ only in
        // case would make the answer ambiguous.
        let e = validate(vec![q("Pick", &["Rust", "rust"])]).unwrap_err();
        assert!(e.contains("twice"), "{e}");
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary() {
        let long = "café ".repeat(200);
        let out = validate(vec![q(&long, &["a", "b"])]).unwrap();
        assert!(out[0].question.chars().count() <= 400);
        // The cut must not have split a multi-byte character.
        assert!(out[0].question.ends_with("café") || out[0].question.ends_with('é')
            || !out[0].question.is_empty());
    }

    #[test]
    fn an_answer_names_the_question_it_answers() {
        let qs = vec![
            Question { header: Some("Language".into()), ..q("Which language?", &["Rust", "Go"]) },
            q("Which database, if any, should this use?", &["Postgres", "None"]),
        ];
        let msg = answer_message(&qs, &[vec!["Rust".into()], vec!["None".into()]]);
        assert!(msg.contains("Language → Rust"));
        // No header: the question's own text stands in, so the thread still
        // reads as an exchange rather than as two loose words.
        assert!(msg.contains("Which database, if any, should this use? → None"));
    }

    #[test]
    fn multi_select_travels_as_camel_case_in_both_directions() {
        // The model sends what the tool schema advertises; the browser reads
        // back what was stored. A mismatch here turns a multiple-choice
        // question into a single-choice one with nothing to show for it.
        let parsed: Question = serde_json::from_str(
            r#"{"question":"Which?","options":[{"label":"a"},{"label":"b"}],"multiSelect":true}"#,
        )
        .unwrap();
        assert!(parsed.multi_select, "multiSelect did not survive the way in");
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back["multiSelect"], serde_json::json!(true));
        assert!(back.get("multi_select").is_none(), "stored under the wrong name");
    }

    #[test]
    fn several_picks_on_one_question_are_joined() {
        let qs = vec![Question { multi_select: true, ..q("Which?", &["a", "b"]) }];
        assert!(answer_message(&qs, &[vec!["a".into(), "b".into()]]).ends_with("a, b"));
    }

    #[test]
    fn answering_nothing_says_so_rather_than_sending_an_empty_turn() {
        let qs = vec![q("Which?", &["a", "b"])];
        assert!(answer_message(&qs, &[vec![]]).contains("best judgement"));
        assert!(answer_message(&qs, &[]).contains("best judgement"));
    }
}
