//! Running an action: the whole of a module's logic layer.
//!
//! Steps come from a closed set, so this is a `match` rather than an
//! interpreter. Nothing an app ships executes — what runs is aichip's own code,
//! doing one of seven named things with arguments the manifest supplied.
//!
//! ## Grants are checked per step, not per action
//!
//! An action that mostly touches the app's own rows should not be blocked by
//! the one step that reaches further, and the refusal should be able to name
//! *which* step stopped. So every step is checked as it comes up, and a step
//! that needs a scope nobody granted stops the action there — with the steps
//! before it already done, because they were allowed and undoing them would
//! mean inventing a transaction across two subsystems.

use super::grants;
use super::manifest::{Manifest, Model, Step};
use super::{expr, App};
use crate::Db;
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionError(pub String);

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ActionError {}

fn bad<T>(m: impl Into<String>) -> Result<T, ActionError> {
    Err(ActionError(m.into()))
}

/// What an action did, for the screen that asked for it.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// Messages from `notify` steps, in order.
    pub messages: Vec<String>,
    /// The view a `goto` step asked for, if any.
    pub goto: Option<String>,
    /// Whether the record the button sat on still exists.
    pub deleted: bool,
    /// A scope the action needed and did not have. The action stopped here.
    pub needs_scope: Option<String>,
}

/// Substitute `{{ … }}` with the expression inside it, evaluated.
///
/// Whole-string substitution rather than a template engine: an expression is
/// the only thing that may appear in the braces, and there is no branching, no
/// looping and no filter syntax. Anything unrecognised is left as it was — a
/// prompt containing a literal `{{` is more likely than a manifest meaning to
/// reference something that does not exist.
pub fn interpolate(template: &str, record: &expr::Record, now: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let src = after[..end].trim();
        match expr::run(src, record, now) {
            Ok(value) => out.push_str(&value.to_string()),
            // Left verbatim, so a broken reference shows up in the prompt a
            // person reads rather than silently becoming an empty string.
            Err(_) => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

fn record_of(model: &Model, row: Option<&Value>) -> expr::Record {
    let mut record = expr::Record::new();
    if let Some(Value::Object(fields)) = row {
        for (key, value) in fields {
            let numeric = model.field(key).is_some_and(|f| {
                matches!(f.ty, super::manifest::FieldType::Decimal)
            });
            record.insert(
                key.clone(),
                match (numeric, value) {
                    (true, Value::String(s)) => {
                        s.parse().map(expr::Val::Num).unwrap_or(expr::Val::Null)
                    }
                    _ => expr::Val::from_json(value),
                },
            );
        }
    }
    record
}

/// Run one action against one record.
pub async fn run(
    db: &Db,
    orchestrator: &crate::Orchestrator,
    app: &App,
    manifest: &Manifest,
    action_name: &str,
    model: &Model,
    row_id: Option<Uuid>,
) -> Result<Outcome, ActionError> {
    let action = manifest
        .action(action_name)
        .ok_or_else(|| ActionError(format!("\"{action_name}\" is not an action of this app")))?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut row = match row_id {
        Some(id) => super::data::get(db, &app.schema, model, id)
            .await
            .map_err(|e| ActionError(e.0))?,
        None => None,
    };
    if row_id.is_some() && row.is_none() {
        return bad("that row is gone");
    }

    let held = grants::of(db, app.id).await.map_err(|e| ActionError(e.to_string()))?;
    let mut out = Outcome::default();

    for step in &action.steps {
        // Checked here, as the step comes up, so the refusal names the step
        // rather than the action and the steps before it still happened.
        if let Some(needed) = step.scope() {
            if !held.contains(&needed) {
                out.needs_scope = Some(needed.as_str().to_string());
                return Ok(out);
            }
            grants::touch(db, app.id, needed).await.ok();
        }

        let record = record_of(model, row.as_ref());
        match step {
            Step::Notify { message } => out.messages.push(interpolate(message, &record, &now)),
            Step::Goto { view } => out.goto = Some(view.clone()),

            Step::Update { values } => {
                let Some(id) = row_id else {
                    return bad("this action changes a record, so it needs one to work on");
                };
                let body = resolved(values, &record, &now);
                row = super::data::update(db, &app.schema, model, id, &body)
                    .await
                    .map_err(|e| ActionError(e.0))?;
                if row.is_none() {
                    return bad("that row is gone");
                }
            }

            Step::Delete => {
                let Some(id) = row_id else {
                    return bad("this action deletes a record, so it needs one to work on");
                };
                super::data::delete(db, &app.schema, model, id)
                    .await
                    .map_err(|e| ActionError(e.0))?;
                out.deleted = true;
                row = None;
            }

            Step::Create { model: target, values } => {
                let target = manifest
                    .model(target)
                    .ok_or_else(|| ActionError(format!("\"{target}\" is not a model")))?;
                super::data::create(db, &app.schema, target, &resolved(values, &record, &now))
                    .await
                    .map_err(|e| ActionError(e.0))?;
            }

            Step::CreateTask { project, title, prompt } => {
                let project_id = resolve_project(db, app, project.as_deref()).await?;
                make_task(
                    db,
                    orchestrator,
                    project_id,
                    &interpolate(title, &record, &now),
                    &interpolate(prompt, &record, &now),
                    false,
                )
                .await?;
                out.messages.push("Added a card to your backlog.".into());
            }

            Step::StartRun { project, prompt, .. } => {
                let project_id = resolve_project(db, app, project.as_deref()).await?;
                let text = interpolate(prompt, &record, &now);
                let title: String = text.chars().take(60).collect();
                make_task(db, orchestrator, project_id, &title, &text, true).await?;
                out.messages.push("Started an agent run.".into());
            }
        }
    }

    Ok(out)
}

/// A step's values as the body `data::create`/`data::update` takes.
///
/// A value the manifest left empty becomes `Value::Null` — which `data::writable`
/// reads as "clear this field", and which is the only way an action can take
/// something back off a row. Interpolating `None` into an empty string instead
/// would write two quotes where a date used to be.
fn resolved(
    values: &[(String, Option<String>)],
    record: &expr::Record,
    now: &str,
) -> Map<String, Value> {
    values
        .iter()
        .map(|(k, v)| {
            let value = match v {
                Some(template) => Value::String(interpolate(template, record, now)),
                None => Value::Null,
            };
            (k.clone(), value)
        })
        .collect()
}

/// Which project a board step means.
///
/// Resolved by name at the moment of the click rather than checked at install,
/// because project names are a property of *this* machine — an app shared with
/// someone else would otherwise refuse to install over a name they do not have.
async fn resolve_project(
    db: &Db,
    app: &App,
    name: Option<&str>,
) -> Result<Uuid, ActionError> {
    let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return bad(
            "this step works on one of your projects, so the manifest has to name one: \
             add `project: <name>` to it",
        );
    };
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM projects
          WHERE workspace_id = $1 AND kind = 'repo' AND lower(name) = lower($2)
          LIMIT 1",
    )
    .bind(app.workspace_id)
    .bind(name)
    .fetch_optional(&db.pool)
    .await
    .map_err(|e| ActionError(e.to_string()))?;

    found.ok_or_else(|| ActionError(format!("there is no project called \"{name}\"")))
}

async fn make_task(
    db: &Db,
    orchestrator: &crate::Orchestrator,
    project_id: Uuid,
    title: &str,
    prompt: &str,
    start: bool,
) -> Result<(), ActionError> {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO tasks (project_id, title, prompt, model_tier, engine, board_column)
         VALUES ($1, $2, $3, 'medium', $4, 'backlog') RETURNING id",
    )
    .bind(project_id)
    .bind(title)
    .bind(prompt)
    .bind(orchestrator.default_engine())
    .fetch_one(&db.pool)
    .await
    .map_err(|e| ActionError(format!("could not add the card: {e}")))?;

    if start {
        orchestrator
            .enqueue_task(id)
            .await
            .map_err(|e| ActionError(format!("the card was added but did not start: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> expr::Record {
        let mut r = expr::Record::new();
        r.insert("description".into(), expr::Val::Str("Coffee".into()));
        r.insert("amount".into(), expr::Val::Num(4.25));
        r.insert("qty".into(), expr::Val::Num(3.0));
        r
    }

    const NOW: &str = "2026-08-02T04:00:00Z";

    #[test]
    fn a_template_reads_the_record() {
        assert_eq!(
            interpolate("Categorise: {{ record.description }}", &record(), NOW),
            "Categorise: Coffee"
        );
        assert_eq!(interpolate("{{ amount * qty }}", &record(), NOW), "12.75");
        assert_eq!(interpolate("on {{ today() }}", &record(), NOW), "on 2026-08-02");
    }

    #[test]
    fn text_without_braces_is_untouched() {
        assert_eq!(interpolate("just a prompt", &record(), NOW), "just a prompt");
        assert_eq!(interpolate("", &record(), NOW), "");
    }

    #[test]
    fn several_substitutions_in_one_string_all_happen() {
        assert_eq!(
            interpolate("{{ description }} x{{ qty }}", &record(), NOW),
            "Coffee x3"
        );
    }

    #[test]
    fn a_broken_reference_stays_visible_rather_than_becoming_nothing() {
        // An empty string in a prompt is a prompt that quietly means something
        // else. Left as it was, it shows up in the text a person reads.
        assert_eq!(
            interpolate("see {{ 1 +++ }}", &record(), NOW),
            "see {{ 1 +++ }}"
        );
        // An unclosed brace is not a substitution at all.
        assert_eq!(interpolate("{{ oops", &record(), NOW), "{{ oops");
    }

    #[test]
    fn a_missing_field_reads_as_nothing_rather_than_failing() {
        // Distinct from the case above: `missing` is valid syntax that
        // evaluates to null, which is a normal half-filled record.
        assert_eq!(interpolate("[{{ missing }}]", &record(), NOW), "[]");
    }

    #[test]
    fn a_value_full_of_braces_cannot_start_a_second_substitution() {
        let mut r = expr::Record::new();
        r.insert("description".into(), expr::Val::Str("{{ amount }}".into()));
        r.insert("amount".into(), expr::Val::Num(99.0));
        // The substituted text is output, not re-scanned — otherwise a row
        // someone typed could reach fields the manifest never named.
        assert_eq!(interpolate("{{ description }}", &r, NOW), "{{ amount }}");
    }
}
