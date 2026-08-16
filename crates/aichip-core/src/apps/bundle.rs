//! Taking an app somewhere else.
//!
//! A bundle is **one JSON document**, not an archive. A module is a manifest
//! and some rows, and both are text — so the thing you send someone is
//! something they can open and read before they install it, which is the same
//! bargain the manifest itself makes. An archive would hide that behind a tool,
//! and buy nothing until a container app has source files to carry.
//!
//! ## Two kinds of export
//!
//! *Share* is the manifest and the shape of its tables, with no rows: what you
//! hand to someone else. *Move* is that plus the data: what you carry to
//! another machine. The choice is made at the point of export rather than
//! guessed at, because "here, try my app" and "put this on my laptop" are
//! different sentences and only one of them means to include your data.
//!
//! ## Import regenerates the schema
//!
//! `schema` in a bundle is **documentation**. Import reads the manifest and
//! derives the DDL again from that, exactly as an install does — so a bundle
//! someone hand-edited cannot introduce a statement that was never in a
//! manifest. There is one code path that makes tables, and this is not a second
//! one.

use super::manifest::{FieldType, Manifest, Model};
use serde_json::{json, Map, Value};

/// The bundle format. Bumped when a reader would get an older one wrong.
pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleError(pub String);

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for BundleError {}

fn bad<T>(m: impl Into<String>) -> Result<T, BundleError> {
    Err(BundleError(m.into()))
}

/// A bundle, read back.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub manifest: String,
    /// Rows per model, in an order safe to insert. Empty for a *Share* export.
    pub rows: Vec<(String, Vec<Map<String, Value>>)>,
}

/// The order models must be written and read in.
///
/// A `ref:` is a real foreign key, so a row pointing at another model needs
/// that model's rows to exist already. Sorted here rather than relying on
/// declaration order, because a manifest may legally declare `line` before
/// `order` — refs resolve after every model is known, precisely so that
/// declaration order is not load bearing.
///
/// A cycle cannot be satisfied by any order, so the remainder is appended as
/// declared and the insert fails honestly rather than looping.
pub fn model_order(models: &[Model]) -> Vec<String> {
    let mut done: Vec<String> = Vec::new();
    let mut left: Vec<&Model> = models.iter().collect();

    while !left.is_empty() {
        let ready: Vec<&Model> = left
            .iter()
            .copied()
            .filter(|m| {
                m.fields.iter().all(|f| match &f.ty {
                    // A self-reference is satisfiable within one table, so it
                    // does not stop the model from being ready.
                    FieldType::Ref(target) => *target == m.name || done.contains(target),
                    _ => true,
                })
            })
            .collect();
        if ready.is_empty() {
            done.extend(left.iter().map(|m| m.name.clone()));
            break;
        }
        for m in ready {
            done.push(m.name.clone());
            left.retain(|x| x.name != m.name);
        }
    }
    done
}

/// Write a bundle.
///
/// `rows` is what a *Move* export carries and what a *Share* export leaves out.
pub fn write(
    manifest_text: &str,
    parsed: &Manifest,
    schema_sql: &str,
    rows: &[(String, Vec<Value>)],
) -> String {
    let ordered = model_order(&parsed.models);
    let data: Map<String, Value> = ordered
        .iter()
        .filter_map(|name| {
            rows.iter()
                .find(|(m, _)| m == name)
                .map(|(_, values)| (name.clone(), Value::Array(values.clone())))
        })
        .collect();

    let doc = json!({
        "format": FORMAT,
        "kind": "aichip-app",
        "app": {
            "name": parsed.name,
            "icon": parsed.icon,
            "summary": parsed.summary,
            "runtime": parsed.runtime.as_str(),
        },
        "manifest": manifest_text,
        // Documentation. Import derives its own from the manifest and never
        // runs this — a hand-edited bundle must not be able to add a statement.
        "schema": schema_sql,
        "modelOrder": ordered,
        "rows": data,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

/// Read a bundle, checking only what a reader must.
///
/// The manifest is *not* parsed here — the caller does that, and gets the
/// parser's own error naming the key. Two places reporting the same problem
/// differently is how a message ends up worse than the one it replaced.
pub fn read(text: &str) -> Result<Bundle, BundleError> {
    let doc: Value = serde_json::from_str(text)
        .map_err(|e| BundleError(format!("this is not a bundle: {e}")))?;

    if doc.get("kind").and_then(Value::as_str) != Some("aichip-app") {
        return bad("this file is not an aichip app bundle");
    }
    match doc.get("format").and_then(Value::as_u64) {
        Some(v) if v as u32 <= FORMAT => {}
        Some(v) => {
            return bad(format!(
                "this bundle is format {v} and this aichip understands up to {FORMAT} — \
                 update aichip to install it"
            ))
        }
        None => return bad("this bundle does not say what format it is"),
    }

    let manifest = doc
        .get("manifest")
        .and_then(Value::as_str)
        .ok_or_else(|| BundleError("this bundle has no manifest in it".into()))?
        .to_string();

    // The order it was written in, so refs land after what they point at. A
    // bundle without one is read in whatever order its rows appear, which is
    // right for the common case of a single model.
    let order: Vec<String> = doc
        .get("modelOrder")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let mut rows: Vec<(String, Vec<Map<String, Value>>)> = Vec::new();
    if let Some(Value::Object(data)) = doc.get("rows") {
        let names: Vec<String> = if order.is_empty() {
            data.keys().cloned().collect()
        } else {
            let mut n = order.clone();
            n.extend(data.keys().filter(|k| !order.contains(k)).cloned());
            n
        };
        for name in names {
            let Some(Value::Array(items)) = data.get(&name) else {
                continue;
            };
            let mut out = Vec::new();
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Object(o) => out.push(o.clone()),
                    _ => return bad(format!("row {i} of \"{name}\" is not a record")),
                }
            }
            rows.push((name, out));
        }
    }

    Ok(Bundle { manifest, rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::manifest;

    fn parsed(yaml: &str) -> Manifest {
        manifest::parse(yaml).expect("test manifest must parse")
    }

    const TWO: &str = "name: T\nmodels:\n  \
                       line: { fields: { order_id: { type: \"ref:order\" } } }\n  \
                       order: { fields: { total: { type: decimal } } }\n";

    #[test]
    fn a_model_comes_after_whatever_it_points_at() {
        // Declared line-then-order, which is legal, so the order has to be
        // worked out rather than taken from the manifest.
        let order = model_order(&parsed(TWO).models);
        let at = |n: &str| order.iter().position(|x| x == n).unwrap();
        assert!(at("order") < at("line"), "{order:?}");
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn a_self_reference_does_not_stall_the_sort() {
        // A tree of pages: satisfiable inside one table, so it must not look
        // like an unsatisfiable dependency.
        let m =
            parsed("name: T\nmodels:\n  page: { fields: { parent: { type: \"ref:page\" } } }\n");
        assert_eq!(model_order(&m.models), vec!["page"]);
    }

    #[test]
    fn a_cycle_is_returned_rather_than_looped_over() {
        // Two models pointing at each other cannot be inserted in any order.
        // The insert should fail saying so, not hang here.
        let m = parsed(
            "name: T\nmodels:\n  a: { fields: { b_id: { type: \"ref:b\" } } }\n  \
             b: { fields: { a_id: { type: \"ref:a\" } } }\n",
        );
        let order = model_order(&m.models);
        assert_eq!(order.len(), 2, "every model still appears: {order:?}");
    }

    #[test]
    fn a_share_export_carries_no_rows() {
        let m = parsed(TWO);
        let text = write("name: T", &m, "-- ddl", &[]);
        let back = read(&text).unwrap();
        assert_eq!(back.manifest, "name: T");
        assert!(back.rows.iter().all(|(_, r)| r.is_empty()));
    }

    #[test]
    fn a_move_export_round_trips_its_rows_in_a_safe_order() {
        let m = parsed(TWO);
        let rows = vec![
            (
                "line".to_string(),
                vec![json!({ "id": "1", "order_id": "9" })],
            ),
            (
                "order".to_string(),
                vec![json!({ "id": "9", "total": "5.00" })],
            ),
        ];
        let back = read(&write("name: T", &m, "", &rows)).unwrap();
        assert_eq!(
            back.rows[0].0, "order",
            "the target has to be written first"
        );
        assert_eq!(back.rows[1].0, "line");
        assert_eq!(back.rows[1].1[0]["order_id"], json!("9"));
        // A decimal is still the string it was, so no digits went through a
        // double on the way out and back.
        assert_eq!(back.rows[0].1[0]["total"], json!("5.00"));
    }

    #[test]
    fn a_bundle_from_the_future_says_so_rather_than_half_working() {
        let text = r#"{"kind":"aichip-app","format":99,"manifest":"name: T"}"#;
        let e = read(text).unwrap_err();
        assert!(e.0.contains("update aichip"), "{e}");
    }

    #[test]
    fn something_that_is_not_a_bundle_is_refused_plainly() {
        assert!(read("not json").unwrap_err().0.contains("not a bundle"));
        assert!(read("{}")
            .unwrap_err()
            .0
            .contains("not an aichip app bundle"));
        assert!(read(r#"{"kind":"aichip-app","format":1}"#)
            .unwrap_err()
            .0
            .contains("no manifest"));
    }

    #[test]
    fn the_bundled_schema_is_documentation_and_nothing_reads_it_back() {
        // The property that stops a hand-edited bundle introducing DDL: what
        // comes out of `read` has no way to express a statement at all.
        let m = parsed(TWO);
        let text = write("name: T", &m, "DROP DATABASE postgres;", &[]);
        assert!(
            text.contains("DROP DATABASE"),
            "it is still written, for a reader"
        );
        let back = read(&text).unwrap();
        assert_eq!(back.manifest, "name: T");
        // Bundle has exactly two fields, and neither is SQL.
        let _: (&String, &Vec<(String, Vec<Map<String, Value>>)>) = (&back.manifest, &back.rows);
    }
}
