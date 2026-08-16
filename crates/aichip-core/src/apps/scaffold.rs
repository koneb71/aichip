//! Asking an agent for a manifest, and telling one how to change an app.
//!
//! Generating a *manifest* is generating text — no tools, no worktree, no files
//! written. The result is returned, not installed: the whole reason a module is
//! YAML rather than code is that a person can read it before it becomes real,
//! and installing it for them would throw that away. Same shape as
//! `/api/agents/generate`.
//!
//! Changing an app is the opposite — a card, a worktree and real files, run by
//! the ordinary orchestrator. [`build_prompt`] is what that card says. For a
//! module it asks for a rewritten manifest and nothing else; for a container it
//! also describes the file contract, because a container app's source is the
//! app and aichip cannot draw it from a declaration.
//!
//! Both prompts are pure functions so what the agent was asked is legible in
//! the source rather than reconstructed from logs. They are also the second
//! copy of the format's documentation, which is why
//! `the_prompt_describes_the_format` checks against the real vocabulary rather
//! than trusting a copy to keep up.

use super::expr::FUNCTIONS;
use super::manifest::Runtime;
use super::runtime as runtimes;
use super::scope::ALL as ALL_SCOPES;

/// What a container runtime's source has to look like for aichip to build it.
///
/// Stated to the agent rather than only to the reader: the Dockerfile is
/// aichip's (see [`super::runtime`]) and the agent never sees it, so the
/// contract between the two is only kept if one side is told what the other
/// expects.
fn file_contract(runtime: Runtime) -> String {
    let files = runtimes::required_files(runtime).join(", ");
    let port = runtimes::port(runtime)
        .map(|p| p.to_string())
        .unwrap_or_default();
    match runtime {
        Runtime::Module => String::new(),
        Runtime::Node => format!(
            "\
* **`server.js` is the entry point** and must listen on `process.env.PORT`
  (aichip sets it to {port}). Do not hardcode a port.
* **Screens are HTML files under `views/`.** `server.js` routes `/` to
  `views/index.html` and `/<name>` to `views/<name>.html`; `/assets/` is
  served as files. Add a screen by adding a file *and* a `menu:` entry in
  `aichip.app.yaml` so it appears in the sidebar. Keep that routing (or
  something equivalent) if you rewrite the server.
* **Every page needs the same two lines in its `<head>`, in this order** —
  copy them from an existing `views/*.html`:

      <script>if (window !== window.parent) document.documentElement.classList.add(\"embedded\");</script>
      <link rel=\"stylesheet\" href=\"/__aichip/app.css\">

  The stylesheet is aichip's own look, served by the dashboard — that is what
  makes a screen part of the app rather than a page in a box, and
  `assets/app.css` after it is where your overrides go. The script tells that
  stylesheet the page is inside the dashboard, which already draws the app's
  name and a tab per screen; a page missing it repeats both. Keep your `<nav>`
  and your `<h1>` in the markup — they are hidden when framed and are the only
  navigation when someone opens the app in its own tab.
* **Dependencies go in `package.json`.** They are installed at build time and
  **nothing is fetched at run time** — the page is served under
  `connect-src 'self'`, so a CDN script tag or a runtime `fetch` to another
  origin is blocked. Vendor it or do without it.
* These files must exist: {files}.
* **Do not write a Dockerfile.** aichip owns the build and will overwrite one.
* Reach your app's own tables through `window.aichip`, which
  `/__aichip/client.js` defines. Do not ask for a database connection; there
  isn't one."
        ),
        Runtime::Static => format!(
            "\
* **`index.html` at the top level is the page.** Everything beside it is served
  as-is by nginx; each screen in the manifest's `menu:` is `<name>.html`
  beside it.
* **Every page needs the same two lines in its `<head>`, in this order** —
  copy them from an existing page:

      <script>if (window !== window.parent) document.documentElement.classList.add(\"embedded\");</script>
      <link rel=\"stylesheet\" href=\"/__aichip/app.css\">

  The stylesheet is aichip's own look, served by the dashboard — that is what
  makes a screen part of the app rather than a page in a box, and
  `assets/app.css` after it is where your overrides go. The script tells that
  stylesheet the page is inside the dashboard, which already draws the app's
  name and a tab per screen; a page missing it repeats both. Keep your `<nav>`
  and your `<h1>` in the markup — they are hidden when framed and are the only
  navigation when someone opens the app in its own tab.
* **Nothing is fetched at run time** — the page is served under
  `connect-src 'self'`, so a CDN script tag or a font from another origin is
  blocked. Inline it or ship the file.
* These files must exist: {files}.
* **Do not write a Dockerfile.** aichip owns the build and will overwrite one.
* Reach your app's own tables through `window.aichip`, which
  `/__aichip/client.js` defines. Do not ask for a database connection; there
  isn't one."
        ),
    }
}

/// The brief for a new manifest.
pub fn manifest_prompt(description: &str, runtime: Runtime) -> String {
    let types = "text, int, decimal, bool, date, datetime, json, ref:<other model>";
    let scopes = ALL_SCOPES
        .iter()
        .map(|s| format!("  {} — {}", s.as_str(), s.blurb()))
        .collect::<Vec<_>>()
        .join("\n");
    let functions = FUNCTIONS.join(", ");
    let runtime_name = runtime.as_str();

    // What the two kinds of app are, in the words the manifest uses. A
    // container app declares models — it gets the same real tables — but
    // declares no views, because it draws its own pages.
    let what_an_app_is = if runtime.is_container() {
        format!(
            "\
An app declares models, which become real Postgres tables it can read and write
through aichip. This one is a **{runtime_name} app**: it also has source code of
its own, which aichip builds into a container and serves in the dashboard. You
are writing only the manifest — but every `menu:` entry you declare becomes a
real HTML screen aichip scaffolds from it, a working CRUD page when the entry
names a `model:`. So declare the tables *and* the screens; the code is
generated from this and refined afterwards. It has no `views:` and no
`actions:` — those belong to declarative apps."
        )
    } else {
        "\
An app is declarative. It has models, which become real Postgres tables, and
views, which aichip's own dashboard draws. No code you write runs — there is no
JavaScript, no Python, no template language. If something cannot be said in the
format below, leave it out rather than inventing syntax for it."
            .to_string()
    };

    // A container app declares no views — offering that vocabulary would be
    // inviting a manifest the parser refuses — but it *does* declare screens,
    // and each menu entry decides what gets scaffolded.
    let views_block = if runtime.is_container() {
        "\n\
menu:
  - {{ label: <what the tab says>, view: <screen_name>, model: <model_name> }}
  # Each entry becomes views/<screen_name>.html. With `model:` it is scaffolded
  # as a working CRUD page for that model; without, an empty page to fill in.
  # Screen names are lower_snake, like every other name here.\n"
            .to_string()
    } else {
        "\n\
views:
  <view_name>:
    kind: list | form | kanban | chart      # may be left out if the view is
    model: <model_name>                     # named for its kind, or if the app
    ...                                     # declares exactly one model
    # list:   columns: [...]   sort: \"-field\"
    # form:   groups: [[a, b], [c]]   buttons: [<action_name>]
    # kanban: group_by: <field>   title: <field>   fields: [...]
    # chart:  shape: bar|line|pie   group_by: <field>   measure: \"sum(field)\"

menu:
  - {{ label: <what the tab says>, view: <view_name> }}\n"
            .to_string()
    };

    // Actions are buttons on a view, so a container app — which draws its own
    // pages — has nowhere to put one and writes its logic in code instead.
    let actions_block = if runtime.is_container() {
        String::new()
    } else {
        "\n\
## Actions, if the app needs a button

actions:
  <action_name>:
    label: <what the button says>
    show_if: \"<expr>\"        # optional
    steps:
      - update: {{ <field>: <value> }}
      - update: {{ <field>: }}     # nothing after the colon clears that field
      - create: {{ model: <model_name>, values: {{ <field>: <value> }} }}
      - delete
      - notify: \"<message>\"
      - goto: <view_name>
      - create_task: {{ title: \"...\", prompt: \"...\" }}     # needs write:board
      - start_run: {{ prompt: \"...\" }}                     # needs run:agents\n"
            .to_string()
    };

    let what_to_build = if runtime.is_container() {
        "\
Declare the tables the app will store things in and a menu of its screens.
Prefer one model over three. Use `compute` for anything derived rather than
storing it twice. Give each model's screen a `model:` so it scaffolds as a
working page — custom behaviour is code, refined afterwards, and none of it
belongs here."
    } else {
        "\
Design the smallest thing that actually does the job. Prefer one model over
three. Give it a list view and, when there is a number worth totalling, a chart.
Use `compute` for anything derived rather than asking a person to type it twice."
    };

    format!(
        r##"Write an aichip app manifest: one YAML document, nothing else.

{what_an_app_is}

## The format

name: <what a person calls it>
icon: "<a single character, e.g. ▤ ◳ ✎ ⏱>"
summary: <one line>
runtime: {runtime_name}
scopes: []            # only if an action needs one; see below

models:
  <model_name>:
    fields:
      <field_name>: {{ type: <type>, required: <bool>, default: "<expr>",
                     compute: "<expr>", label: "<what to show>" }}
    indexes: [<field_name>]

{views_block}
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
{actions_block}
A step that needs a scope only works if the manifest lists it under `scopes`,
and a person then has to grant it. Ask for nothing you can do without:

{scopes}

## What to build

{what_to_build}

Reply with the YAML alone — no prose before it, no explanation after it, and no
code fence unless you cannot help it.

The app to build:
{description}"##
    )
}

/// What a card that changes an existing app says.
///
/// Unlike [`manifest_prompt`] this one runs with tools in a worktree, so it has
/// to say what to *edit* rather than what to reply with — and for a container
/// app it carries the file contract, because there the source is the app.
pub fn build_prompt(manifest: &str, runtime: Runtime, brief: &str) -> String {
    let file = super::MANIFEST_FILE;
    let contract = file_contract(runtime);
    let what_to_do = if runtime.is_container() {
        format!(
            "\
This is a **{}** app: its source is in this folder and aichip builds it into a
container. Change the code. Change `{file}` too if — and only if — the app needs
a table it does not have.

## The file contract

{contract}",
            runtime.as_str()
        )
    } else {
        format!(
            "\
This is a **module**: the whole app is `{file}`, and aichip's dashboard draws it.
**No code you write runs.** Editing that one file is the entire job — do not add
JavaScript, a server, a Dockerfile or a build step, because nothing would
execute them.

**There is nothing to run, build, install or test, so do not use a shell.** No
command can tell you whether this is right: aichip parses the manifest when your
change lands and puts the parser's own message — which names the offending key —
on the app's page. Edit the file, and when it is edited, stop.

Field types are exactly `text, int, decimal, bool, date, datetime, json,
ref:<model>`; unknown keys are refused rather than ignored; `id`, `created_at`
and `updated_at` are added to every table for you and must not be declared."
        )
    };

    format!(
        r##"Change this aichip app.

{what_to_do}

## What is being asked for

{brief}

## How this lands

Your changes are merged onto the app's `main` branch automatically when this
card completes, and the app updates itself. There is no review step, so leave
the app working: a `{file}` that does not parse takes every screen down until
someone fixes it by hand.

Adding a table or a nullable column applies itself. **Removing or retyping
anything waits for a person to approve the SQL**, so prefer adding to
rewriting — a change that drops a column leaves the app running on the old
schema until someone reads the migration.

## The manifest as it stands

```yaml
{manifest}
```"##
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
            return text
                .lines()
                .skip(i)
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
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
        // the real vocabulary rather than a copy of it, and for every runtime,
        // because the parser is the same one for all three.
        for runtime in [Runtime::Module, Runtime::Node, Runtime::Static] {
            let p = manifest_prompt("anything", runtime);
            for ty in [
                "text", "int", "decimal", "bool", "date", "datetime", "json", "ref:",
            ] {
                assert!(
                    p.contains(ty),
                    "{runtime:?}'s prompt never mentions the {ty} type"
                );
            }
            for f in FUNCTIONS {
                assert!(p.contains(f), "{runtime:?}'s prompt never mentions {f}()");
            }
            for scope in ALL_SCOPES {
                assert!(
                    p.contains(scope.as_str()),
                    "{runtime:?}'s prompt never mentions {scope}"
                );
            }
            for reserved in manifest::RESERVED_FIELDS {
                assert!(
                    p.contains(reserved),
                    "{runtime:?}'s prompt never says {reserved} is taken"
                );
            }
            assert!(
                p.contains("anything"),
                "the description has to reach the model"
            );
            assert!(
                p.contains(&format!("runtime: {}", runtime.as_str())),
                "{runtime:?}'s prompt asks for the wrong runtime"
            );
        }
    }

    #[test]
    fn the_prompt_names_every_step_and_view_kind() {
        let p = manifest_prompt("x", Runtime::Module);
        for step in [
            "update",
            "create",
            "delete",
            "notify",
            "goto",
            "create_task",
            "start_run",
        ] {
            assert!(
                p.contains(step),
                "the prompt never mentions the {step} step"
            );
        }
        for kind in ["list", "form", "kanban", "chart"] {
            assert!(p.contains(kind), "the prompt never mentions {kind} views");
        }
    }

    #[test]
    fn a_container_app_is_offered_screens_but_not_views_or_actions() {
        // It draws its own pages, so a `views:` or `actions:` block would be a
        // manifest the parser refuses — and offering the syntax is how an
        // agent comes to write one. `menu:` it now gets: each entry is a
        // screen the skeleton scaffolds into a real HTML page.
        for runtime in [Runtime::Node, Runtime::Static] {
            let p = manifest_prompt("x", runtime);
            assert!(!p.contains("\nviews:"), "{runtime:?} was offered views");
            assert!(!p.contains("\nactions:"), "{runtime:?} was offered actions");
            assert!(
                p.contains("\nmenu:"),
                "{runtime:?} was not told about screens"
            );
            assert!(
                p.contains("model:"),
                "{runtime:?} was not told model: selects CRUD"
            );
            // Models it does get: the tables are the same tables.
            assert!(p.contains("\nmodels:"));
        }
    }

    #[test]
    fn a_build_tells_a_container_app_what_the_dockerfile_expects() {
        // aichip owns the build and the agent never sees it, so this prompt is
        // the only place the two sides are made to agree.
        let p = build_prompt("name: T", Runtime::Node, "add a page");
        assert!(
            p.contains("process.env.PORT"),
            "a hardcoded port serves nothing"
        );
        for f in runtimes::required_files(Runtime::Node) {
            assert!(p.contains(f), "the prompt never mentions {f}");
        }
        assert!(p.contains("Do not write a Dockerfile"));
        assert!(p.contains("add a page"), "the brief has to reach the model");
        assert!(
            p.contains("name: T"),
            "the agent needs the manifest as it stands"
        );
        // The screen convention: an agent that doesn't know views/ exists will
        // bolt new pages onto server.js and the sidebar never learns of them.
        assert!(
            p.contains("views/"),
            "the node contract never mentions the views/ layout"
        );
        assert!(
            p.contains("menu:"),
            "the contract never says screens are declared"
        );

        let s = build_prompt("name: T", Runtime::Static, "x");
        assert!(s.contains("index.html"));
        assert!(!s.contains("server.js"), "a static app has no server");
    }

    #[test]
    fn a_build_tells_a_module_that_nothing_it_writes_will_run() {
        // The failure this prevents is expensive and silent: an agent writes a
        // React app into a module's folder and the card completes green having
        // produced nothing that can ever execute.
        let p = build_prompt("name: T", Runtime::Module, "add a total");
        assert!(p.contains(super::super::MANIFEST_FILE));
        assert!(p.contains("No code you write runs"));
        assert!(!p.contains("Dockerfile") || p.contains("do not add"));
        // And that there is nothing to verify with. Observed: an agent whose
        // edit was already correct spent the rest of a paid run trying to
        // parse the YAML with python, ruby and node in turn.
        assert!(p.contains("do not use a shell"));
        // And it says what landing means, because there is no review step.
        assert!(p.contains("automatically"));
        assert!(
            p.contains("waits for a person"),
            "the schema gate has to be predictable"
        );
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
