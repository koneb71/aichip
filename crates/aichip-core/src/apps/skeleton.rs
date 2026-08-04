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
  res.end(
    "<!doctype html><meta charset=\"utf-8\"><body style=\"font: 15px system-ui; margin: 3rem\">" +
      "<h1>No such screen.</h1><p>This app has: " +
      screens.map((s) => '<a href="/' + s + '">' + s + "</a>").join(", ") +
      "</p></body>"
  );
});

server.listen(process.env.PORT || 3000);
"#;

const CSS: &str = r#"/* Written by aichip. The look of every scaffolded screen — edit freely. */
* { box-sizing: border-box; }
body { font: 15px/1.5 system-ui, sans-serif; color: #26251f; margin: 0; }
main { max-width: 56rem; margin: 0 auto; padding: 1.5rem; }
nav { display: flex; gap: 1rem; padding: 0.75rem 1.5rem; border-bottom: 1px solid #e4e2da; }
nav a { color: #6b6a63; text-decoration: none; font-size: 13px; }
nav a.current { color: #26251f; font-weight: 600; }
h1 { font-size: 1.3rem; }
table { width: 100%; border-collapse: collapse; margin-top: 1rem; }
th { text-align: left; font-size: 12px; color: #6b6a63; border-bottom: 1px solid #e4e2da; padding: 0.4rem 0.5rem; }
td { border-bottom: 1px solid #f0efe9; padding: 0.4rem 0.5rem; }
form.add { display: flex; flex-wrap: wrap; gap: 0.5rem; margin-top: 1rem; align-items: end; }
form.add label { display: flex; flex-direction: column; font-size: 12px; color: #6b6a63; gap: 0.2rem; }
input, select { font: inherit; padding: 0.3rem 0.5rem; border: 1px solid #d7d5cc; border-radius: 6px; }
button { font: inherit; padding: 0.35rem 0.9rem; border: 0; border-radius: 6px; background: #5a54f0; color: white; cursor: pointer; }
button.quiet { background: none; color: #a09e94; padding: 0.2rem 0.4rem; }
button.quiet:hover { color: #c0392b; }
.empty { color: #6b6a63; margin-top: 1.5rem; }
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

fn page(manifest: &Manifest, current: Option<&str>, title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n\
         <link rel=\"stylesheet\" href=\"/assets/app.css\">\n\
         <script src=\"/__aichip/client.js\"></script>\n\
         </head>\n<body>\n{}\n<main>\n{}\n</main>\n</body>\n</html>\n",
        esc(title),
        nav(manifest, current),
        body
    )
}

/// The home screen: where the screens are, and how much is in each table.
fn index_page(manifest: &Manifest) -> String {
    let mut body = format!("<h1>{}</h1>", esc(&manifest.name));
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
            "<h1>{}</h1>\n<p class=\"empty\">This screen is yours to write — edit \
             <code>views/{}.html</code>, or ask aichip to change this app.</p>",
            esc(&entry.label),
            entry.view
        );
        return page(manifest, Some(&entry.view), &entry.label, &body);
    };
    page(manifest, Some(&entry.view), &entry.label, &crud_body(&entry.label, model))
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

fn crud_body(title: &str, model: &Model) -> String {
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
        r#"<h1>{title}</h1>
{form}
<table>
  <thead><tr>{heads}<th></th></tr></thead>
  <tbody id="rows"></tbody>
</table>
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
        title = esc(title),
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
    #[test]
    fn nothing_emitted_names_another_origin() {
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
