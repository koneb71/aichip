//! Asking an agent for a manifest, and making a person read it.
//!
//! A module is a declaration, so generating one is generating text — no tools,
//! no worktree, no files written. The result is *returned*, not installed: the
//! whole reason a module is YAML rather than code is that a person can read it
//! before it becomes real, and installing it for them would throw that away.
//! Same shape as `/api/agents/generate`.
//!
//! The prompt is a pure function so what the agent was asked is legible in the
//! source rather than reconstructed from logs. It is also the second copy of
//! the format's documentation, which is why `the_prompt_describes_the_format`
//! checks it against the real vocabulary rather than trusting it to keep up.

use super::expr::FUNCTIONS;
use super::scope::ALL as ALL_SCOPES;

/// The full brief handed to the model.
pub fn prompt(description: &str) -> String {
    let types = "text, int, decimal, bool, date, datetime, json, ref:<other model>";
    let scopes = ALL_SCOPES
        .iter()
        .map(|s| format!("  {} — {}", s.as_str(), s.blurb()))
        .collect::<Vec<_>>()
        .join("\n");
    let functions = FUNCTIONS.join(", ");

    format!(
        r##"Write an aichip app manifest: one YAML document, nothing else.

An app is declarative. It has models, which become real Postgres tables, and
views, which aichip's own dashboard draws. No code you write runs — there is no
JavaScript, no Python, no template language. If something cannot be said in the
format below, leave it out rather than inventing syntax for it.

## The format

name: <what a person calls it>
icon: "<a single character, e.g. ▤ ◳ ✎ ⏱>"
summary: <one line>
runtime: module
scopes: []            # only if an action needs one; see below

models:
  <model_name>:
    fields:
      <field_name>: {{ type: <type>, required: <bool>, default: "<expr>",
                     compute: "<expr>", label: "<what to show>" }}
    indexes: [<field_name>]

views:
  <view_name>:
    kind: list | form | kanban | chart      # may be left out if the view is
    model: <model_name>                     # named for its kind, or if the app
    ...                                     # declares exactly one model
    # list:   columns: [...]   sort: "-field"
    # form:   groups: [[a, b], [c]]   buttons: [<action_name>]
    # kanban: group_by: <field>   title: <field>   fields: [...]
    # chart:  shape: bar|line|pie   group_by: <field>   measure: "sum(field)"

menu:
  - {{ label: <what the tab says>, view: <view_name> }}

## Rules that will be enforced

* **Field types are exactly these**: {types}. There are no others — no `email`,
  no `enum`, no `array`. A choice field is `text`.
* **Names** are lower-case letters, digits and underscores, starting with a
  letter. That applies to models, fields, views and actions.
* **`id`, `created_at` and `updated_at` are added to every table for you.** Do
  not declare them. You may still show them in a view.
* **Unknown keys are refused**, so do not invent one. A misspelling is an error,
  not something quietly ignored.
* **A field cannot have both `default` and `compute`** — a computed field is
  rewritten on every save, so the default could never be seen.
* Every field a view names must exist. Every view a menu names must exist.

## Expressions

`default`, `compute` and an action's `show_if` take a small expression: field
names, numbers, quoted strings, `true`/`false`/`null`, `+ - * / %`,
`== != < <= > >=`, `&& || !`, brackets, and these functions: {functions}.

Nothing else — no method calls, no `if`, no property access beyond a bare field
name. `today()` and `now()` are how you get the date.

## Actions, if the app needs a button

actions:
  <action_name>:
    label: <what the button says>
    show_if: "<expr>"        # optional
    steps:
      - update: {{ <field>: <value> }}
      - create: {{ model: <model_name>, values: {{ <field>: <value> }} }}
      - delete
      - notify: "<message>"
      - goto: <view_name>
      - create_task: {{ title: "...", prompt: "..." }}     # needs write:board
      - start_run: {{ prompt: "..." }}                     # needs run:agents

A step that needs a scope only works if the manifest lists it under `scopes`,
and a person then has to grant it. Ask for nothing you can do without:

{scopes}

## What to build

Design the smallest thing that actually does the job. Prefer one model over
three. Give it a list view and, when there is a number worth totalling, a chart.
Use `compute` for anything derived rather than asking a person to type it twice.

Reply with the YAML alone — no prose before it, no explanation after it, and no
code fence unless you cannot help it.

The app to build:
{description}"##
    )
}

/// Pull the manifest out of whatever the model replied with.
///
/// Models fence YAML about half the time and apologise before it more often
/// than that. Both are recoverable and neither is worth failing a paid call
/// over, so the fence is stripped and anything before the first top-level key
/// is dropped.
pub fn extract(output: &str) -> String {
    let text = output.trim();

    // A fenced block wins outright when there is one.
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        // Skip the language tag on the fence line, if any.
        let body = after.split_once('\n').map_or(after, |(_, rest)| rest);
        if let Some(end) = body.find("```") {
            return body[..end].trim().to_string();
        }
    }

    // Otherwise drop any preamble: the first line that looks like a top-level
    // key is where the document starts.
    for (i, line) in text.lines().enumerate() {
        let is_key = line
            .split_once(':')
            .is_some_and(|(k, _)| !k.is_empty() && !k.starts_with(char::is_whitespace));
        if is_key {
            return text.lines().skip(i).collect::<Vec<_>>().join("\n").trim().to_string();
        }
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::manifest;

    #[test]
    fn the_prompt_describes_the_format_it_will_be_judged_against() {
        // Two descriptions of one format drift, and the symptom is an agent
        // confidently writing a manifest the parser refuses. Checked against
        // the real vocabulary rather than a copy of it.
        let p = prompt("anything");
        for ty in ["text", "int", "decimal", "bool", "date", "datetime", "json", "ref:"] {
            assert!(p.contains(ty), "the prompt never mentions the {ty} type");
        }
        for f in FUNCTIONS {
            assert!(p.contains(f), "the prompt never mentions {f}()");
        }
        for scope in ALL_SCOPES {
            assert!(p.contains(scope.as_str()), "the prompt never mentions {scope}");
        }
        for reserved in manifest::RESERVED_FIELDS {
            assert!(p.contains(reserved), "the prompt never says {reserved} is taken");
        }
        assert!(p.contains("anything"), "the description has to reach the model");
    }

    #[test]
    fn the_prompt_names_every_step_and_view_kind() {
        let p = prompt("x");
        for step in ["update", "create", "delete", "notify", "goto", "create_task", "start_run"] {
            assert!(p.contains(step), "the prompt never mentions the {step} step");
        }
        for kind in ["list", "form", "kanban", "chart"] {
            assert!(p.contains(kind), "the prompt never mentions {kind} views");
        }
    }

    #[test]
    fn a_fenced_reply_loses_its_fence() {
        assert_eq!(extract("```yaml\nname: T\n```"), "name: T");
        assert_eq!(extract("```\nname: T\n```"), "name: T");
        assert_eq!(
            extract("Sure! Here you go:\n\n```yaml\nname: T\nicon: \"x\"\n```\nHope that helps."),
            "name: T\nicon: \"x\""
        );
    }

    #[test]
    fn an_unfenced_reply_loses_its_preamble() {
        assert_eq!(
            extract("Here is the manifest you asked for.\n\nname: T\nsummary: s"),
            "name: T\nsummary: s"
        );
        // And a reply that is already clean is untouched.
        assert_eq!(extract("name: T\nsummary: s"), "name: T\nsummary: s");
    }

    #[test]
    fn what_comes_out_of_a_realistic_reply_actually_parses() {
        // The end-to-end property: extract feeds the parser, so a reply the
        // model plausibly gives has to survive the trip.
        let reply = "Certainly. Here's a manifest for that:\n\n```yaml\n\
                     name: Reading list\nicon: \"▤\"\nsummary: Books to read\n\
                     runtime: module\nmodels:\n  book:\n    fields:\n      \
                     title: { type: text, required: true }\n      \
                     pages: { type: int }\n      \
                     done: { type: bool }\nviews:\n  list:\n    columns: [title, pages, done]\n\
                     menu:\n  - { label: Books, view: list }\n```\n\nLet me know if you'd like changes.";
        let m = manifest::parse(&extract(reply)).expect("a plausible reply must parse");
        assert_eq!(m.name, "Reading list");
        assert_eq!(m.models.len(), 1);
        assert_eq!(m.views.len(), 1);
    }
}
