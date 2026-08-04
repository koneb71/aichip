//! The tree a container app is created with, derived from its manifest.
//!
//! Odoo's trick, borrowed whole: the skeleton is scaffolded **from the spec**,
//! not written by an agent. A manifest declares models and a `menu:` of
//! screens; install materialises `server.js` as a router and one
//! `views/<screen>.html` per entry — a working CRUD page when the entry names
//! a model. So a generated app *works the moment it installs*, for free, and
//! the paid agent runs are spent on behaviour, not boilerplate.
//!
//! This replaces `runtime::starter()`. The split with [`super::runtime`] is
//! deliberate: that module is the builds aichip owns (Dockerfiles, ports,
//! drift); this one is the tree an app starts from. The invariant tying them —
//! the tree satisfies the Dockerfile that will build it — is tested here
//! against `runtime::required_files`, same as it was there.
//!
//! Two rules every emitted file keeps:
//!
//! * **Nothing interpolated is free text.** Model and screen names came
//!   through the manifest's `ident()` (lower-case, digits, underscores), and
//!   field labels are HTML-escaped. The charset is the defence, as everywhere
//!   identifiers travel.
//! * **Nothing is fetched at run time.** The pages run under
//!   `connect-src 'self'`; the only script they load is `/__aichip/client.js`,
//!   which is same-origin by construction. A test greps every emitted byte for
//!   `http://` / `https://` to keep it that way.

use super::manifest::{FieldType, Manifest, MenuItem, Model, Runtime};

/// The files a new app's folder starts with. Paths may contain `/`; the
/// caller creates directories.
pub fn files(manifest: &Manifest) -> Vec<(String, String)> {
    match manifest.runtime {
        Runtime::Module => Vec::new(),
        Runtime::Node => {
            let mut out = vec![
                ("package.json".to_string(), PACKAGE.to_string()),
                ("server.js".to_string(), SERVER.to_string()),
                ("assets/app.css".to_string(), CSS.to_string()),
                ("views/index.html".to_string(), index_page(manifest)),
            ];
            for entry in &manifest.menu {
                out.push((format!("views/{}.html", entry.view), screen_page(manifest, entry)));
            }
            out
        }
        Runtime::Static => {
            // Same pages, no server: nginx serves the folder, so screens live
            // at `/<name>.html` and index.html is the front door.
            let mut out = vec![
                ("assets/app.css".to_string(), CSS.to_string()),
                ("index.html".to_string(), index_page(manifest)),
            ];
            for entry in &manifest.menu {
                out.push((format!("{}.html", entry.view), screen_page(manifest, entry)));
            }
            out
        }
    }
}

const PACKAGE: &str = r#"{
  "name": "aichip-app",
  "private": true,
  "version": "0.1.0",
  "main": "server.js",
  "dependencies": {}
}
"#;

/// The router. Dependency-free on purpose — a floor an agent replaces, not a
/// framework someone has to learn to delete.
const SERVER: &str = r#"// Written by aichip from your manifest. Replace freely; keep two invariants:
//   * listen on process.env.PORT — that is the port aichip proxies to
//   * fetch nothing from another origin — the pages run under connect-src 'self'
//
// Routing: /            -> views/index.html
//          /<name>      -> views/<name>.html   (lower-case, digits, underscores)
//          /assets/<f>  -> assets/<f>
// Add a screen by adding a file under views/ and a menu: entry in
// aichip.app.yaml so it appears in the sidebar.
const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");

const TYPES = {
  html: "text/html; charset=utf-8",
  css: "text/css",
  js: "text/javascript",
  svg: "image/svg+xml",
  png: "image/png",
};

const server = http.createServer((req, res) => {
  const raw = new URL(req.url, "http://app").pathname;
  let file = null;
  if (raw === "/") file = path.join(__dirname, "views", "index.html");
  else if (/^\/assets\/[a-z0-9_.-]+$/.test(raw)) file = path.join(__dirname, raw);
  else if (/^\/[a-z0-9_]+$/.test(raw)) file = path.join(__dirname, "views", raw.slice(1) + ".html");

  if (file && fs.existsSync(file)) {
    res.writeHead(200, { "content-type": TYPES[file.split(".").pop()] || "text/plain" });
    res.end(fs.readFileSync(file));
    return;
  }

  const screens = fs
    .readdirSync(path.join(__dirname, "views"))
    .filter((f) => f.endsWith(".html"))
    .map((f) => f.slice(0, -5));
  res.writeHead(404, { "content-type": "text/html; charset=utf-8" });
  // Wears the same head as a real screen: this is reachable from a menu entry
  // naming a file nobody wrote, and unstyled text sitting on the dashboard's
  // surface reads as aichip being broken rather than as a missing page.
  res.end(
    '<!doctype html><meta charset="utf-8">' +
      '<script>if (window !== window.parent) document.documentElement.classList.add("embedded");</script>' +
      '<link rel="stylesheet" href="/__aichip/app.css">' +
      "<body><main><h1>No such screen.</h1><p>This app has: " +
      screens.map((s) => '<a href="/' + s + '">' + s + "</a>").join(", ") +
      "</p></main></body>"
  );
});

server.listen(process.env.PORT || 3000);
"#;

/// aichip's own look, served to every app at `/__aichip/app.css`.
///
/// Not written into the app's folder, for the same reason `client.js` is not:
/// an app should look like part of the dashboard *as the dashboard changes*,
/// and a copy scaffolded once is a copy that drifts the first time the theme
/// moves. The app's own `assets/app.css` loads after this one and overrides
/// whatever it wants.
///
/// The values mirror `web/src/index.css`'s `@theme` block. They are duplicated
/// rather than shared because the two sides are a Rust string and a Tailwind
/// directive with no build step between them — so the rule is that this file
/// follows that one, and a colour that looks subtly off is the symptom.
pub const THEME: &str = r#"/* aichip's look, served by the dashboard. Not a file in your app: override it
   in assets/app.css rather than editing it, since you cannot. */
:root {
  --surface: #f7f7f8;
  --panel: #ffffff;
  --line: #e7e7ea;
  --ink: #191a1f;
  --ink-dim: #6d7180;
  --accent: #4f46e5;
  --danger: #dc2626;
}

* { box-sizing: border-box; }
html, body { margin: 0; }
body {
  font: 15px/1.5 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  color: var(--ink);
  background: var(--surface);
}

/* Inside aichip the shell already supplies the frame: the sidebar names the
   app and the tab bar names the screen, so a second nav and a second title are
   the app announcing itself inside something that already did. They stay in
   the markup and are hidden here, because the same page is reachable in its
   own tab — where they are the only navigation there is. */
.embedded body { background: transparent; }
.embedded nav { display: none; }
.embedded main { padding: 0; }
.embedded main > h1:first-child { display: none; }

nav {
  display: flex;
  gap: 1rem;
  align-items: center;
  padding: 0.7rem 1.5rem;
  border-bottom: 1px solid var(--line);
  background: var(--panel);
}
nav a { color: var(--ink-dim); text-decoration: none; font-size: 13px; }
nav a.current { color: var(--ink); font-weight: 600; }

main { max-width: 56rem; margin: 0 auto; padding: 1.5rem; }
h1 { font-size: 1.15rem; font-weight: 600; margin: 0 0 0.75rem; }
p { margin: 0.5rem 0; }
a { color: var(--accent); }
.empty { color: var(--ink-dim); }

/* A panel, the same shape the dashboard draws its own in. */
.card {
  background: var(--panel);
  border: 1px solid var(--line);
  border-radius: 12px;
  box-shadow: 0 1px 2px rgba(16, 17, 20, 0.05);
  overflow: hidden;
}

table { width: 100%; border-collapse: collapse; font-size: 14px; }
th {
  text-align: left;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--ink-dim);
  padding: 0.6rem 0.9rem;
  border-bottom: 1px solid var(--line);
}
td { padding: 0.55rem 0.9rem; border-bottom: 1px solid var(--line); }
tbody tr:last-child td { border-bottom: 0; }

form.add {
  display: flex;
  flex-wrap: wrap;
  gap: 0.6rem;
  align-items: end;
  margin-bottom: 1rem;
}
form.add label {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--ink-dim);
}
input, select, textarea {
  font: inherit;
  color: var(--ink);
  background: var(--panel);
  padding: 0.35rem 0.6rem;
  border: 1px solid var(--line);
  border-radius: 8px;
  outline: none;
}
input:focus, select:focus, textarea:focus { border-color: var(--accent); }
button {
  font: inherit;
  font-size: 13px;
  padding: 0.4rem 0.9rem;
  border: 0;
  border-radius: 8px;
  background: var(--accent);
  color: white;
  cursor: pointer;
}
button.quiet { background: none; color: var(--ink-dim); padding: 0.15rem 0.4rem; }
button.quiet:hover { color: var(--danger); }
ul { padding-left: 1.1rem; }
li { margin: 0.3rem 0; }
"#;

const CSS: &str = r#"/* Your app's own styles.
 *
 * aichip's look arrives from /__aichip/app.css, which the dashboard serves and
 * keeps current — this file loads after it, so anything here wins. Put app
 * specific styling in here rather than trying to restyle the theme.
 */
"#;

/// Minimal escaping for the one place free text lands in markup: labels.
fn esc(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The label rule, matching the module renderer's `fieldLabel`:
/// `spent_on` reads as "Spent on" — not title case, which looks like a header
/// rather than a name for a thing.
fn label_of(name: &str, label: &Option<String>) -> String {
    if let Some(l) = label {
        return l.clone();
    }
    let words = name.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => words,
    }
}

/// The `<input>` type for a field, mirroring the module renderer's choices.
fn input_type(ty: &FieldType) -> &'static str {
    match ty {
        FieldType::Int | FieldType::Decimal => "number",
        FieldType::Date => "date",
        FieldType::Datetime => "datetime-local",
        _ => "text",
    }
}

/// The screen's path, per runtime — the same rule the dashboard's
/// `screenPath` uses, stated here for the nav links inside the pages.
fn href(runtime: Runtime, view: &str) -> String {
    match runtime {
        Runtime::Static => format!("/{view}.html"),
        _ => format!("/{view}"),
    }
}

fn nav(manifest: &Manifest, current: Option<&str>) -> String {
    let mut out = String::from("<nav>");
    let home = match manifest.runtime {
        Runtime::Static => "/index.html",
        _ => "/",
    };
    out.push_str(&format!(
        "<a href=\"{home}\"{}>{}</a>",
        if current.is_none() { " class=\"current\"" } else { "" },
        esc(&manifest.name)
    ));
    for entry in &manifest.menu {
        out.push_str(&format!(
            "<a href=\"{}\"{}>{}</a>",
            href(manifest.runtime, &entry.view),
            if current == Some(entry.view.as_str()) { " class=\"current\"" } else { "" },
            esc(&entry.label)
        ));
    }
    out.push_str("</nav>");
    out
}

/// One page: aichip's theme, the app's own stylesheet, the bridge client, and
/// the content.
///
/// The `embedded` class is set in `<head>`, before the body exists, so the nav
/// and title a framed page does not want are never painted — a class applied
/// after load would show them for a frame and then snatch them away.
///
/// `window !== window.parent` is the whole test. It cannot be spoofed *into*
/// being wrong in a way that matters: at worst a page opened standalone
/// decides it is embedded and hides its own nav, which is a cosmetic mistake
/// in the app's own tab.
fn page(manifest: &Manifest, current: Option<&str>, title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n\
         <script>if (window !== window.parent) document.documentElement.classList.add(\"embedded\");</script>\n\
         <link rel=\"stylesheet\" href=\"/__aichip/app.css\">\n\
         <link rel=\"stylesheet\" href=\"/assets/app.css\">\n\
         <script src=\"/__aichip/client.js\"></script>\n\
         </head>\n<body>\n{nav}\n<main>\n<h1>{title}</h1>\n{body}\n</main>\n</body>\n</html>\n",
        title = esc(title),
        nav = nav(manifest, current),
        body = body
    )
}

/// The home screen: where the screens are, and how much is in each table.
fn index_page(manifest: &Manifest) -> String {
    let mut body = String::new();
    if !manifest.summary.is_empty() {
        body.push_str(&format!("<p>{}</p>", esc(&manifest.summary)));
    }
    if manifest.menu.is_empty() {
        body.push_str(
            "<p class=\"empty\">This app has no screens yet. Add a <code>menu:</code> entry \
             to <code>aichip.app.yaml</code>, or ask aichip to change this app.</p>",
        );
    } else {
        body.push_str("<ul>");
        for entry in &manifest.menu {
            let count = match &entry.model {
                Some(m) => format!(" — <span id=\"count-{m}\"></span>"),
                None => String::new(),
            };
            body.push_str(&format!(
                "<li><a href=\"{}\">{}</a>{}</li>",
                href(manifest.runtime, &entry.view),
                esc(&entry.label),
                count
            ));
        }
        body.push_str("</ul>");
        // `total` from a limit-1 list is the cheap way to a row count; the
        // element ids are model names, which are ident()-clean.
        let models: Vec<&str> = manifest
            .menu
            .iter()
            .filter_map(|e| e.model.as_deref())
            .collect();
        if !models.is_empty() {
            body.push_str("<script>\n");
            for m in models {
                body.push_str(&format!(
                    "aichip.list(\"{m}\", {{ limit: 1 }}).then(function (r) {{\n  \
                     document.getElementById(\"count-{m}\").textContent = r.total + \" rows\";\n}});\n"
                ));
            }
            body.push_str("</script>");
        }
    }
    page(manifest, None, &manifest.name, &body)
}

/// One screen. With a model: a live CRUD page. Without: a titled stub.
fn screen_page(manifest: &Manifest, entry: &MenuItem) -> String {
    let Some(model) = entry
        .model
        .as_deref()
        .and_then(|m| manifest.models.iter().find(|declared| declared.name == m))
    else {
        let body = format!(
            "<p class=\"empty\">This screen is yours to write — edit \
             <code>views/{}.html</code>, or ask aichip to change this app.</p>",
            entry.view
        );
        return page(manifest, Some(&entry.view), &entry.label, &body);
    };
    page(manifest, Some(&entry.view), &entry.label, &crud_body(model))
}

/// Fields a person types into: not computed (recalculated on every write, so
/// sending one is refused) and not json (no sensible single input).
fn typed_fields(model: &Model) -> Vec<&super::manifest::Field> {
    model
        .fields
        .iter()
        .filter(|f| f.compute.is_none() && f.ty != FieldType::Json)
        .collect()
}

fn crud_body(model: &Model) -> String {
    let fields = typed_fields(model);

    let mut form = String::from("<form class=\"add\" id=\"add\">\n");
    for f in &fields {
        form.push_str(&format!(
            "  <label>{}<input name=\"{}\" type=\"{}\"{}></label>\n",
            esc(&label_of(&f.name, &f.label)),
            f.name,
            input_type(&f.ty),
            if f.required && f.default.is_none() { " required" } else { "" },
        ));
    }
    form.push_str("  <button>Add</button>\n</form>");

    let heads: String = model
        .fields
        .iter()
        .map(|f| format!("<th>{}</th>", esc(&label_of(&f.name, &f.label))))
        .collect::<Vec<_>>()
        .join("");
    let cols: String = model
        .fields
        .iter()
        .map(|f| format!("\"{}\"", f.name))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"{form}
<div class="card">
  <table>
    <thead><tr>{heads}<th></th></tr></thead>
    <tbody id="rows"></tbody>
  </table>
</div>
<p class="empty" id="empty" hidden>Nothing here yet.</p>
<script>
var MODEL = "{model_name}";
var COLS = [{cols}];

function cell(value) {{
  var td = document.createElement("td");
  td.textContent = value === null || value === undefined ? "" : String(value);
  return td;
}}

function load() {{
  aichip.list(MODEL, {{ order: "-created_at", limit: 200 }}).then(function (r) {{
    var rows = document.getElementById("rows");
    rows.textContent = "";
    document.getElementById("empty").hidden = r.rows.length > 0;
    r.rows.forEach(function (row) {{
      var tr = document.createElement("tr");
      COLS.forEach(function (c) {{ tr.appendChild(cell(row[c])); }});
      var td = document.createElement("td");
      var del = document.createElement("button");
      del.className = "quiet";
      del.textContent = "✕";
      del.addEventListener("click", function () {{
        aichip.remove(MODEL, row.id).then(load);
      }});
      td.appendChild(del);
      tr.appendChild(td);
      rows.appendChild(tr);
    }});
  }});
}}

document.getElementById("add").addEventListener("submit", function (e) {{
  e.preventDefault();
  var values = {{}};
  new FormData(e.target).forEach(function (v, k) {{ if (v !== "") values[k] = v; }});
  aichip.create(MODEL, values).then(function () {{
    e.target.reset();
    load();
  }}).catch(function (err) {{ alert(err.message); }});
}});

load();
</script>"#,
        model_name = model.name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::manifest::parse;

    fn tracker() -> Manifest {
        parse(
            "name: Tracker\nruntime: node\nsummary: Tasks by status\n\
             models:\n  task:\n    fields:\n      \
             title: { type: text, required: true }\n      \
             done_on: { type: date }\n      \
             hours: { type: decimal }\n      \
             meta: { type: json }\n      \
             derived: { type: text, compute: \"upper(title)\" }\n\
             menu:\n  - { label: Tasks, view: tasks, model: task }\n  - { view: about }\n",
        )
        .unwrap()
    }

    fn body_of<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
        &files.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no {name}")).1
    }

    #[test]
    fn every_menu_entry_becomes_a_screen_file() {
        let files = files(&tracker());
        assert!(files.iter().any(|(n, _)| n == "views/tasks.html"));
        assert!(files.iter().any(|(n, _)| n == "views/about.html"));
        assert!(files.iter().any(|(n, _)| n == "views/index.html"));

        // Static: same screens at the top level, no server.
        let mut m = tracker();
        m.runtime = Runtime::Static;
        let files = super::files(&m);
        assert!(files.iter().any(|(n, _)| n == "tasks.html"));
        assert!(files.iter().all(|(n, _)| n != "server.js"));
    }

    #[test]
    fn the_tree_satisfies_the_dockerfile_that_will_build_it() {
        // Moved from runtime.rs when starter() became this module: the
        // invariant is that the tree aichip creates satisfies the Dockerfile
        // aichip wrote — `COPY package*.json ./` fails outright when nothing
        // matches, and nginx serving a folder with no index is a 403.
        for runtime in [Runtime::Static, Runtime::Node] {
            let mut m = tracker();
            m.runtime = runtime;
            let files = super::files(&m);
            for needed in super::super::runtime::required_files(runtime) {
                assert!(
                    files.iter().any(|(name, _)| name == needed),
                    "a new {runtime:?} app has no {needed}, so its first build would fail"
                );
            }
            for (name, body) in &files {
                assert!(!body.trim().is_empty(), "{runtime:?}'s {name} is empty");
            }
        }
        assert!(super::files(&parse("name: M\n").unwrap()).is_empty(), "a module has no tree");
    }

    #[test]
    fn a_manifest_with_no_menu_still_builds_and_says_what_to_do() {
        // The floor stays a floor: no screens is not an error, it is an app
        // with one page explaining how to get screens.
        let m = parse("name: Bare\nruntime: node\n").unwrap();
        let files = files(&m);
        assert!(body_of(&files, "views/index.html").contains("no screens yet"));
    }

    #[test]
    fn the_server_listens_on_the_port_it_is_given() {
        // Moved from runtime.rs: a server hardcoding 3000 works until the
        // Dockerfile's ENV changes, then serves nothing with no error to read.
        let files = files(&tracker());
        assert!(body_of(&files, "server.js").contains("process.env.PORT"));

        let package: serde_json::Value = serde_json::from_str(body_of(&files, "package.json"))
            .expect("package.json has to be JSON npm can read");
        assert_eq!(package["main"], "server.js");
        // Nothing to install is what makes the first build need no network.
        assert_eq!(package["dependencies"].as_object().unwrap().len(), 0);
    }

    /// The CSP invariant. The pages run under `connect-src 'self'`, so a
    /// skeleton that shipped an absolute URL — a CDN script, a font, anything —
    /// would render a page that silently fails. Never delete this test.
    /// The theme's colours are a copy of the dashboard's, and a copy with no
    /// check is a copy that drifts — silently, into an app that looks *almost*
    /// like aichip, which is worse than one that plainly does not.
    ///
    /// Read rather than `include_str!`d so a checkout without the web tree
    /// (or a crate published on its own) skips instead of failing to compile:
    /// this pins a relationship between two files, and the relationship only
    /// exists when both are here.
    #[test]
    fn the_theme_uses_the_dashboards_own_colours() {
        let css = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/index.css");
        let Ok(dashboard) = std::fs::read_to_string(&css) else {
            return;
        };
        // Every colour THEME declares must be one the dashboard declares too.
        for line in THEME.lines() {
            let Some((name, value)) = line.trim().split_once(':') else { continue };
            let value = value.trim().trim_end_matches(';');
            if !name.starts_with("--") || !value.starts_with('#') {
                continue;
            }
            assert!(
                dashboard.contains(value),
                "{name}: {value} is not a colour the dashboard uses — the app theme has drifted"
            );
        }
    }

    #[test]
    fn nothing_emitted_names_another_origin() {
        // THEME is not written into the app's folder, but every page links it,
        // so an @import or a webfont in there is the same hole this exists to
        // close — and it would be caught nowhere else.
        assert!(
            !THEME.contains("http://") && !THEME.contains("https://"),
            "the served theme names another origin, which connect-src 'self' refuses"
        );
        for runtime in [Runtime::Node, Runtime::Static] {
            let mut m = tracker();
            m.runtime = runtime;
            for (name, body) in files(&m) {
                // `http://app` in server.js is the URL-parsing base for
                // relative paths, never fetched; nothing else may appear.
                let cleaned = body.replace("http://app\"", "");
                assert!(
                    !cleaned.contains("http://") && !cleaned.contains("https://"),
                    "{name} names another origin, which connect-src 'self' will refuse"
                );
            }
        }
    }

    #[test]
    fn a_crud_screen_speaks_through_the_bridge_client() {
        let files = files(&tracker());
        let tasks = body_of(&files, "views/tasks.html");
        assert!(tasks.contains("/__aichip/client.js"));
        assert!(tasks.contains("aichip.list(MODEL"));
        assert!(tasks.contains("aichip.create(MODEL"));
        assert!(tasks.contains("aichip.remove(MODEL"));
        // The form skips what a person cannot type: computed fields are
        // refused by the writer, json has no single input.
        assert!(!tasks.contains("name=\"derived\""));
        assert!(!tasks.contains("name=\"meta\""));
        // …but the table still shows every declared column.
        assert!(tasks.contains("<th>Derived</th>"));
        // Labels follow the module renderer's rule: spent_on -> "Spent on".
        assert!(tasks.contains("<th>Done on</th>"));
        // Input types follow the field types.
        assert!(tasks.contains("name=\"done_on\" type=\"date\""));
        assert!(tasks.contains("name=\"hours\" type=\"number\""));
    }

    /// The whole point of the theme being served rather than scaffolded: an
    /// app has to look like part of the dashboard, and keep looking like it
    /// when the dashboard changes.
    #[test]
    fn a_screen_wears_aichips_look_and_can_still_override_it() {
        let files = files(&tracker());
        let tasks = body_of(&files, "views/tasks.html");
        // Order matters: the app's own sheet loads last, so it wins.
        let theme = tasks.find("/__aichip/app.css").expect("no theme");
        let own = tasks.find("/assets/app.css").expect("no app stylesheet");
        assert!(theme < own, "the app could not override a theme that loads after it");

        // And the scaffolded file is an override point, not a copy of the
        // theme — a copy would be frozen at the moment it was written.
        let css = body_of(&files, "assets/app.css");
        assert!(!css.contains("--surface"), "the theme was copied into the app");
    }

    /// Embedded, the dashboard already names the app and the screen; the page
    /// must not say either a second time. Standalone — the "open in a new tab"
    /// link — it is the only navigation there is, so the markup keeps both and
    /// the theme hides them.
    #[test]
    fn a_framed_page_drops_the_chrome_the_dashboard_already_draws() {
        let files = files(&tracker());
        let tasks = body_of(&files, "views/tasks.html");

        // Decided in <head>, before the body paints — a class applied on load
        // would show the nav for a frame and then snatch it away.
        let flag = tasks.find("window !== window.parent").expect("no embed test");
        assert!(flag < tasks.find("<body").unwrap(), "the nav would flash before hiding");
        assert!(tasks.contains("document.documentElement.classList.add(\"embedded\")"));

        // Both are present in the markup…
        assert!(tasks.contains("<nav>"));
        // …and the title is <main>'s FIRST element child, which is what the
        // selector below keys on. Asserting the two halves separately let an
        // element slip in front of the h1 with every test still green.
        assert!(
            tasks.contains("<main>\n<h1>Tasks</h1>"),
            "an element before the h1 would stop `main > h1:first-child` matching"
        );
        // …and both are hidden by the theme when framed, along with the page
        // background, so the dashboard's surface shows through.
        for rule in [
            ".embedded nav { display: none; }",
            ".embedded main > h1:first-child { display: none; }",
            ".embedded body { background: transparent; }",
        ] {
            assert!(THEME.contains(rule), "the theme never hides: {rule}");
        }
    }

    #[test]
    fn a_screen_without_a_model_is_a_stub_that_says_so() {
        let files = files(&tracker());
        let about = body_of(&files, "views/about.html");
        assert!(about.contains("yours to write"));
        assert!(!about.contains("aichip.list"));
    }

    #[test]
    fn labels_are_escaped_on_their_way_into_markup() {
        // Labels are the one free-text thing that lands in HTML. Everything
        // else interpolated (model names, screen names, field names) came
        // through ident() and has nothing to escape.
        let m = parse(
            "name: \"A <b> app\"\nruntime: node\n\
             models:\n  t:\n    fields:\n      x: { type: text, label: \"a < b\" }\n\
             menu:\n  - { label: \"<script>\", view: t_screen, model: t }\n",
        )
        .unwrap();
        for (_, body) in files(&m) {
            assert!(!body.contains("<script>alert"), "sanity");
            assert!(!body.contains("<b> app"));
            assert!(!body.contains("<th>a < b"));
        }
        let files = files(&m);
        assert!(body_of(&files, "views/t_screen.html").contains("&lt;script&gt;"));
    }

    #[test]
    fn the_router_only_serves_names_the_manifest_could_declare() {
        // The route pattern and the manifest ident() charset must stay the
        // same set — a screen the parser accepts must be one the router
        // serves, and nothing outside it resolves to a file.
        let files = files(&tracker());
        assert!(body_of(&files, "server.js").contains(r"/^\/[a-z0-9_]+$/"));
    }
}
