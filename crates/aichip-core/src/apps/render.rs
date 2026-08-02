//! What the dashboard needs in order to draw an app.
//!
//! The manifest, projected into JSON, plus the one query a chart needs. The
//! browser never sees YAML: it is handed an already-validated shape, so the
//! renderer has no parsing to do and no way to disagree with the server about
//! what the app declares.

use super::manifest::{parse_measure, Agg, Manifest, Model, ViewSpec};
use super::query::{self, Raw};
use crate::Db;
use serde_json::{json, Value};

/// Everything about an app that a screen needs.
pub fn manifest_json(m: &Manifest) -> Value {
    json!({
        "name": m.name,
        "icon": m.icon,
        "summary": m.summary,
        "runtime": m.runtime.as_str(),
        "scopes": m.scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        "models": m.models.iter().map(|model| json!({
            "name": model.name,
            "fields": model.fields.iter().map(|f| json!({
                "name": f.name,
                "label": f.label,
                "type": f.ty.as_str(),
                "required": f.required,
                // Both are sent so a form can grey out a computed input and
                // say why, rather than letting someone type into a field whose
                // value is about to be overwritten.
                "computed": f.compute.is_some(),
                "hasDefault": f.default.is_some(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "views": m.views.iter().map(view_json).collect::<Vec<_>>(),
        "actions": m.actions.iter().map(|a| json!({
            "name": a.name,
            "label": a.label,
            // The raw text: the browser evaluates it, which is why there are
            // two implementations of the expression language and one corpus.
            "showIf": a.show_if,
            "steps": a.steps.iter().map(|s| json!({
                "kind": s.kind(),
                "scope": s.scope().map(|sc| sc.as_str()),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "menu": m.menu.iter().map(|i| json!({ "label": i.label, "view": i.view }))
            .collect::<Vec<_>>(),
    })
}

fn view_json(v: &super::manifest::View) -> Value {
    let spec = match &v.spec {
        ViewSpec::List { columns, sort } => json!({
            "columns": columns,
            "sort": sort.as_ref().map(|s| json!({ "field": s.field, "descending": s.descending })),
        }),
        ViewSpec::Form { groups, buttons } => json!({ "groups": groups, "buttons": buttons }),
        ViewSpec::Kanban { group_by, title, fields } => {
            json!({ "groupBy": group_by, "title": title, "fields": fields })
        }
        ViewSpec::Chart { kind, group_by, measure } => {
            json!({ "shape": kind.as_str(), "groupBy": group_by, "measure": measure })
        }
    };
    json!({ "name": v.name, "kind": v.kind.as_str(), "model": v.model, "spec": spec })
}

/// One bar, line point or slice.
///
/// Run in Postgres rather than by adding up rows here: a chart over a year of
/// entries would otherwise mean shipping a year of entries to draw twelve bars.
pub async fn chart(
    db: &Db,
    schema: &str,
    model: &Model,
    group_by: &str,
    measure: &str,
    raw: &Raw,
) -> Result<Vec<Value>, String> {
    let (agg, field) = parse_measure(measure)?;
    let (group, _) = super::query::parse(model, &Raw { filters: vec![format!("{group_by}:notnull")], ..Default::default() })
        .map(|_| (group_by.to_string(), ()))
        .map_err(|e| e.0)?;

    // Every identifier below is checked against the model before it is used —
    // the group column by the filter parse above, the measured column here —
    // so nothing in this statement came from a request.
    let measured = match &field {
        None => "count(*)".to_string(),
        Some(name) => {
            let declared = model
                .field(name)
                .map(|f| f.name.clone())
                .or_else(|| {
                    super::manifest::RESERVED_FIELDS
                        .contains(&name.as_str())
                        .then(|| name.clone())
                })
                .ok_or_else(|| format!("\"{name}\" is not a field of \"{}\"", model.name))?;
            let col = format!("t.{}", query::quote(&declared));
            match agg {
                Agg::Count => format!("count({col})"),
                Agg::Sum => format!("sum({col})"),
                Agg::Avg => format!("avg({col})"),
                Agg::Min => format!("min({col})"),
                Agg::Max => format!("max({col})"),
            }
        }
    };

    let parsed = super::query::parse(model, raw).map_err(|e| e.0)?;
    let frag = query::where_clause(&parsed, 1);
    // The caller's ordering and paging are about rows, not about buckets.
    let where_only = frag.sql.split(" ORDER BY").next().unwrap_or("").to_string();

    let sql = format!(
        "SELECT t.{group_col}::text AS bucket, ({measured})::text AS value
           FROM {schema_q}.{table} t{where_only}
          GROUP BY t.{group_col}
          ORDER BY 1 ASC
          LIMIT 200",
        group_col = query::quote(&group),
        schema_q = query::quote(schema),
        table = query::quote(&model.name),
    );

    let mut q = sqlx::query(&sql);
    for param in &frag.params {
        q = q.bind(param);
    }
    use sqlx::Row;
    let rows = q
        .fetch_all(&db.pool)
        .await
        .map_err(|e| format!("could not chart \"{}\": {e}", model.name))?;

    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "bucket": r.get::<Option<String>, _>("bucket"),
                "value": r.get::<Option<String>, _>("value"),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::manifest;

    #[test]
    fn a_measure_reads_its_aggregate_and_field() {
        assert_eq!(parse_measure("sum(total)").unwrap(), (Agg::Sum, Some("total".into())));
        assert_eq!(parse_measure(" avg( amount ) ").unwrap(), (Agg::Avg, Some("amount".into())));
        assert_eq!(parse_measure("count()").unwrap(), (Agg::Count, None));
        assert_eq!(parse_measure("count(id)").unwrap(), (Agg::Count, Some("id".into())));
    }

    #[test]
    fn a_measure_that_is_not_one_says_what_was_expected() {
        for bad in ["total", "sum total", "sum(", "median(x)", "sum()"] {
            let e = parse_measure(bad).unwrap_err();
            assert!(!e.is_empty(), "{bad} was accepted");
        }
        assert!(parse_measure("median(x)").unwrap_err().contains("count, sum, avg"));
    }

    #[test]
    fn a_measure_naming_a_field_that_does_not_exist_is_refused_at_install() {
        // Otherwise the chart is empty and nothing says why.
        let e = manifest::parse(
            "name: T\nmodels:\n  t: { fields: { a: { type: int } } }\n\
             views:\n  chart: { group_by: a, measure: \"sum(nope)\" }\n",
        )
        .unwrap_err();
        assert_eq!(e.at, "views.chart.measure");
        assert!(e.message.contains("\"nope\""), "{e}");
    }

    #[test]
    fn the_manifest_projection_tells_a_form_what_it_may_not_edit() {
        let m = manifest::parse(
            "name: T\nmodels:\n  t:\n    fields:\n      a: { type: int }\n      \
             b: { type: int, compute: \"a * 2\" }\n",
        )
        .unwrap();
        let json = manifest_json(&m);
        let fields = json["models"][0]["fields"].as_array().unwrap();
        assert_eq!(fields[0]["computed"], serde_json::json!(false));
        assert_eq!(fields[1]["computed"], serde_json::json!(true));
    }

    #[test]
    fn every_action_step_declares_the_scope_it_needs() {
        // The browser greys out a button whose scope is missing, so it has to
        // be told which scope that is rather than guessing from the label.
        let m = manifest::parse(
            "name: T\nscopes: [run:agents]\nmodels:\n  t: { fields: { a: { type: int } } }\n\
             actions:\n  go:\n    steps:\n      - start_run: { prompt: p }\n",
        )
        .unwrap();
        let json = manifest_json(&m);
        assert_eq!(json["actions"][0]["steps"][0]["scope"], serde_json::json!("run:agents"));
    }
}
