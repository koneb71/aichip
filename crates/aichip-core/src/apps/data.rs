//! An app's own rows: reading them, and writing them back.
//!
//! Everything an app can do to its tables goes through here, and everything
//! here is built from the manifest rather than from the request. The statement
//! shape comes from the declared model, the identifiers come from the declared
//! model, and every value the caller supplied is a bound parameter — see
//! [`super::query`], which does the deciding and has the tests.
//!
//! Values are bound as text and cast in SQL (`$1::numeric`). One binding path
//! for every type is one path to get right, and it keeps a money column off the
//! floating-point round trip that `numeric`-as-a-JSON-number would force.

use super::manifest::{Field, FieldType, Manifest, Model, RESERVED_FIELDS};
use super::query::{self, Query, Raw};
use crate::Db;
use serde_json::{Map, Value};
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataError(pub String);

impl std::fmt::Display for DataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for DataError {}

fn bad<T>(message: impl Into<String>) -> Result<T, DataError> {
    Err(DataError(message.into()))
}

/// Find a declared model, or say which names exist.
pub fn model_of<'a>(manifest: &'a Manifest, name: &str) -> Result<&'a Model, DataError> {
    manifest.model(name).ok_or_else(|| {
        DataError(format!(
            "\"{name}\" is not a model this app declares — it has {}",
            if manifest.models.is_empty() {
                "none".to_string()
            } else {
                manifest
                    .models
                    .iter()
                    .map(|m| format!("\"{}\"", m.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ))
    })
}

/// Turn a JSON body into the columns and values to write.
///
/// Only declared fields survive. A key the model does not have is refused
/// rather than dropped: an app sending `{"amout": 5}` has a typo, and silently
/// storing nothing is how that typo lives for a month. The three columns aichip
/// owns are refused too — `updated_at` is ours to set, not the caller's to
/// claim.
///
/// Computed fields are refused for the same reason: a value someone sends for a
/// field whose value is derived would be overwritten on the next save anyway,
/// so accepting it would be a lie.
///
/// ## `None` is an explicit NULL, not an omission
///
/// Those were the same thing here until they were not: a null arrived, the
/// field was optional, and it was skipped — so "clear the check-in time" left
/// the old timestamp on the row and reported success. Silently doing nothing is
/// the worst of the three possible answers; the other two are doing it and
/// refusing.
///
/// A required field is still refused, which is the whole reason `required`
/// exists given the column itself is nullable — see the note at the top of
/// `schema.rs` on why `required` is not `NOT NULL`.
fn writable(
    model: &Model,
    body: &Map<String, Value>,
) -> Result<Vec<(String, Option<String>)>, DataError> {
    let mut out = Vec::new();
    for (key, value) in body {
        if RESERVED_FIELDS.contains(&key.as_str()) {
            return bad(format!("\"{key}\" is set by aichip and cannot be written"));
        }
        let Some(field) = model.field(key) else {
            return bad(format!("\"{key}\" is not a field of \"{}\"", model.name));
        };
        if field.compute.is_some() {
            return bad(format!(
                "\"{key}\" is computed from other fields, so it cannot be set directly"
            ));
        }
        if value.is_null() {
            if field.required {
                return bad(format!("\"{key}\" is required"));
            }
            out.push((field.name.clone(), None));
            continue;
        }
        out.push((field.name.clone(), Some(as_text(&field.ty, value)?)));
    }
    Ok(out)
}

/// A JSON value as the text Postgres will cast.
///
/// ## Decimals should be sent as strings
///
/// A JSON number goes through an `f64` on the way in — that is what the parser
/// does, without the `arbitrary_precision` feature — so a value with more
/// significant digits than a double can hold arrives already rounded. `10.15`
/// survives, because the shortest representation of that double is `10.15`
/// again; `12345678901234567890.12` does not.
///
/// So a decimal sent as a **string** is taken verbatim and is exact, and a
/// decimal sent as a number is accepted but is only as good as a double. This
/// is the same bargain every money API makes, and it round-trips: the read
/// projection returns `numeric` as text, so a row an app reads and writes back
/// keeps every digit.
fn as_text(ty: &FieldType, value: &Value) -> Result<String, DataError> {
    let text = match (ty, value) {
        (FieldType::Json, v) => return Ok(v.to_string()),
        (_, Value::String(s)) => s.clone(),
        (_, Value::Number(n)) => n.to_string(),
        (_, Value::Bool(b)) => b.to_string(),
        (_, other) => return bad(format!("{other} is not a {}", ty.as_str())),
    };

    // Checked here so the message can name the field's type. The cast in the
    // statement would refuse it too, but Postgres would only say "numeric".
    let ok = match ty {
        FieldType::Int => text.parse::<i64>().is_ok(),
        FieldType::Decimal => is_decimal(&text),
        FieldType::Bool => matches!(text.as_str(), "true" | "false"),
        FieldType::Ref(_) => Uuid::parse_str(&text).is_ok(),
        // Dates are left to Postgres, which knows far more about what a date is
        // than anything worth writing here.
        _ => true,
    };
    if ok {
        return Ok(text);
    }
    // A number too big for a double stringifies to exponent notation, which
    // Postgres would accept — and storing it would mean storing the rounded
    // value without saying so. Refusing is right; saying only "not a decimal"
    // about something that plainly looks like one is not.
    if *ty == FieldType::Decimal && value.is_number() && text.contains(['e', 'E']) {
        return bad(format!(
            "{value} has more digits than a JSON number can carry, so it arrived \
             already rounded to {text}. Send it as a string to keep every digit."
        ));
    }
    bad(format!("\"{text}\" is not a {}", ty.as_str()))
}

/// Whether this is a plain decimal literal.
///
/// Deliberately not `parse::<f64>()`, which is the very rounding this exists to
/// let callers avoid — and which would also accept `inf` and `NaN`, neither of
/// which is a `numeric`.
fn is_decimal(text: &str) -> bool {
    let body = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (whole, frac) = match body.split_once('.') {
        Some((w, f)) => (w, f),
        None => (body, ""),
    };
    !body.is_empty()
        && !(whole.is_empty() && frac.is_empty())
        && whole.chars().all(|c| c.is_ascii_digit())
        && frac.chars().all(|c| c.is_ascii_digit())
}

/// The clock, as a string the expression language can hand back.
fn now_text() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Fill in defaults and computed fields.
///
/// Defaults only apply on the way in and only when the caller said nothing;
/// computed fields are recalculated on **every** write, which is what makes
/// them trustworthy — a total that is only right when someone remembers to
/// resend it is not a total.
///
/// A computed value that comes out null is stored as null rather than refused.
/// A row whose amount is not filled in yet has no total yet, and that is an
/// ordinary state rather than a broken manifest.
fn derive(
    model: &Model,
    values: &mut Vec<(String, Option<String>)>,
    existing: Option<&Value>,
    creating: bool,
) -> Result<(), DataError> {
    let now = now_text();

    if creating {
        for field in &model.fields {
            if field.compute.is_some() || values.iter().any(|(n, _)| n == &field.name) {
                continue;
            }
            let Some(src) = &field.default else { continue };
            let value = super::expr::run(src, &super::expr::Record::new(), &now).map_err(|e| {
                DataError(format!("the default for \"{}\" failed: {e}", field.name))
            })?;
            if value != super::expr::Val::Null {
                values.push((field.name.clone(), Some(value.to_string())));
            }
        }
    }

    // The record a compute sees: what is already stored, overlaid with what is
    // being written now. Both, because an update naming only `qty` must still
    // be able to compute `amount * qty` from the amount already there.
    let computed: Vec<&Field> = model
        .fields
        .iter()
        .filter(|f| f.compute.is_some())
        .collect();
    if computed.is_empty() {
        return Ok(());
    }
    let mut record = super::expr::Record::new();
    if let Some(Value::Object(row)) = existing {
        for (k, v) in row {
            record.insert(k.clone(), coerce(model, k, v));
        }
    }
    for (name, text) in values.iter() {
        let val = match text {
            Some(text) => text_as_val(model, name, text),
            None => super::expr::Val::Null,
        };
        record.insert(name.clone(), val);
    }

    for field in computed {
        let src = field.compute.as_ref().expect("filtered to computed fields");
        let value = super::expr::run(src, &record, &now)
            .map_err(|e| DataError(format!("\"{}\" could not be worked out: {e}", field.name)))?;
        values.retain(|(n, _)| n != &field.name);
        // A compute that now works out to nothing writes NULL rather than being
        // left out. On an update, leaving it out means the previous answer
        // stays on the row — a derived value that no longer follows from the
        // fields it is derived from.
        values.push((
            field.name.clone(),
            (value != super::expr::Val::Null).then(|| value.to_string()),
        ));
    }
    Ok(())
}

/// A stored JSON value as the expression language sees it.
///
/// Decimals come back from the projection as strings so no digits are lost on
/// the wire; arithmetic needs them as numbers again.
fn coerce(model: &Model, name: &str, v: &Value) -> super::expr::Val {
    let numeric = model
        .field(name)
        .is_some_and(|f| matches!(f.ty, FieldType::Decimal));
    match (numeric, v) {
        (true, Value::String(s)) => s
            .parse::<f64>()
            .map(super::expr::Val::Num)
            .unwrap_or(super::expr::Val::Null),
        _ => super::expr::Val::from_json(v),
    }
}

fn text_as_val(model: &Model, name: &str, text: &str) -> super::expr::Val {
    match model.field(name).map(|f| &f.ty) {
        Some(FieldType::Int | FieldType::Decimal) => text
            .parse::<f64>()
            .map(super::expr::Val::Num)
            .unwrap_or(super::expr::Val::Null),
        Some(FieldType::Bool) => super::expr::Val::Bool(text == "true"),
        _ => super::expr::Val::Str(text.to_string()),
    }
}

fn missing_required(model: &Model, present: &[(String, Option<String>)]) -> Option<String> {
    model
        .fields
        .iter()
        // Present-but-cleared counts as missing. `writable` refuses an explicit
        // null on a required field, so this only catches one arriving another
        // way — a default expression that worked out to nothing.
        .find(|f| {
            f.required
                && f.compute.is_none()
                && !present.iter().any(|(n, v)| n == &f.name && v.is_some())
        })
        .map(|f| f.name.clone())
}

/// Rows matching a query.
pub async fn list(
    db: &Db,
    schema: &str,
    model: &Model,
    raw: &Raw,
) -> Result<Vec<Value>, DataError> {
    let query = query::parse(model, raw).map_err(|e| DataError(e.0))?;
    let frag = query::where_clause(&query, 1);
    let sql = format!(
        "SELECT {} AS row FROM {}.{} t{}",
        query::projection(model),
        query::quote(schema),
        query::quote(&model.name),
        frag.sql
    );

    let mut q = sqlx::query(&sql);
    for param in &frag.params {
        q = q.bind(param);
    }
    let rows = q
        .fetch_all(&db.pool)
        .await
        .map_err(|e| DataError(format!("could not read \"{}\": {e}", model.name)))?;
    Ok(rows.iter().map(|r| r.get::<Value, _>("row")).collect())
}

/// How many rows match, ignoring limit and offset.
pub async fn count(db: &Db, schema: &str, model: &Model, raw: &Raw) -> Result<i64, DataError> {
    let query = query::parse(model, raw).map_err(|e| DataError(e.0))?;
    // The same filters, but the ordering and paging are meaningless for a
    // count and `ORDER BY` on a count is a wasted sort.
    let bare = Query {
        order: None,
        limit: 1,
        offset: 0,
        ..query
    };
    let frag = query::where_clause(&bare, 1);
    let where_only = frag.sql.split(" ORDER BY").next().unwrap_or("").to_string();
    let sql = format!(
        "SELECT count(*) FROM {}.{} t{}",
        query::quote(schema),
        query::quote(&model.name),
        where_only
    );
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    for param in &frag.params {
        q = q.bind(param);
    }
    q.fetch_one(&db.pool)
        .await
        .map_err(|e| DataError(format!("could not count \"{}\": {e}", model.name)))
}

pub async fn get(
    db: &Db,
    schema: &str,
    model: &Model,
    id: Uuid,
) -> Result<Option<Value>, DataError> {
    let sql = format!(
        "SELECT {} AS row FROM {}.{} t WHERE t.\"id\" = $1::uuid",
        query::projection(model),
        query::quote(schema),
        query::quote(&model.name)
    );
    let row = sqlx::query(&sql)
        .bind(id.to_string())
        .fetch_optional(&db.pool)
        .await
        .map_err(|e| DataError(format!("could not read \"{}\": {e}", model.name)))?;
    Ok(row.map(|r| r.get::<Value, _>("row")))
}

pub async fn create(
    db: &Db,
    schema: &str,
    model: &Model,
    body: &Map<String, Value>,
) -> Result<Value, DataError> {
    let mut values = writable(model, body)?;
    derive(model, &mut values, None, true)?;
    if let Some(name) = missing_required(model, &values) {
        return bad(format!("\"{name}\" is required"));
    }

    let mut columns: Vec<String> = Vec::new();
    let mut holes: Vec<String> = Vec::new();
    let mut params: Vec<Option<String>> = Vec::new();
    for (name, text) in &values {
        let ty = &model
            .field(name)
            .expect("writable only returns declared fields")
            .ty;
        columns.push(query::quote(name));
        holes.push(format!("${}::{}", params.len() + 1, query::cast(ty)));
        params.push(text.clone());
    }

    // A model whose every field was left out is still a row: the three columns
    // aichip fills are enough to make one, and DEFAULT VALUES is how Postgres
    // spells an insert with nothing in it.
    let sql = if columns.is_empty() {
        format!(
            "INSERT INTO {}.{} DEFAULT VALUES RETURNING id",
            query::quote(schema),
            query::quote(&model.name)
        )
    } else {
        format!(
            "INSERT INTO {}.{} ({}) VALUES ({}) RETURNING id",
            query::quote(schema),
            query::quote(&model.name),
            columns.join(", "),
            holes.join(", ")
        )
    };

    let mut q = sqlx::query_scalar::<_, Uuid>(&sql);
    for param in &params {
        // `as_deref` rather than the `String`: `None` must reach Postgres as
        // NULL, which is the difference between clearing a field and writing
        // the four characters "null" into it.
        q = q.bind(param.as_deref());
    }
    let id = q
        .fetch_one(&db.pool)
        .await
        .map_err(|e| DataError(format!("could not add to \"{}\": {e}", model.name)))?;

    get(db, schema, model, id)
        .await?
        .ok_or_else(|| DataError("the row vanished immediately after being written".into()))
}

pub async fn update(
    db: &Db,
    schema: &str,
    model: &Model,
    id: Uuid,
    body: &Map<String, Value>,
) -> Result<Option<Value>, DataError> {
    let mut values = writable(model, body)?;
    if values.is_empty() {
        return get(db, schema, model, id).await;
    }
    // The row as it stands, so a compute can see the fields this write is not
    // touching. Read before the change, since that is what its inputs are.
    let existing = get(db, schema, model, id).await?;
    if existing.is_none() {
        return Ok(None);
    }
    derive(model, &mut values, existing.as_ref(), false)?;

    let mut sets: Vec<String> = Vec::new();
    let mut params: Vec<Option<String>> = Vec::new();
    for (name, text) in &values {
        let ty = &model
            .field(name)
            .expect("writable only returns declared fields")
            .ty;
        sets.push(format!(
            "{} = ${}::{}",
            query::quote(name),
            params.len() + 1,
            query::cast(ty)
        ));
        params.push(text.clone());
    }
    // Set here rather than by a trigger: one place writes rows, so one place
    // can stamp them, and a trigger would be a second thing to keep in step
    // with every table this creates.
    sets.push("\"updated_at\" = now()".into());

    let sql = format!(
        "UPDATE {}.{} SET {} WHERE \"id\" = ${}::uuid",
        query::quote(schema),
        query::quote(&model.name),
        sets.join(", "),
        params.len() + 1
    );
    let mut q = sqlx::query(&sql);
    for param in &params {
        q = q.bind(param.as_deref());
    }
    let done = q
        .bind(id.to_string())
        .execute(&db.pool)
        .await
        .map_err(|e| DataError(format!("could not change \"{}\": {e}", model.name)))?;
    if done.rows_affected() == 0 {
        return Ok(None);
    }
    get(db, schema, model, id).await
}

/// Write a row exactly as it was exported, ids and timestamps included.
///
/// Only for reading aichip's own bundles back. A `ref:` column holds the id of
/// another row, so re-minting ids on import would quietly break every link the
/// bundle carried — which is a worse outcome than the narrow exception this
/// makes to "the three columns aichip owns cannot be written".
///
/// It is still not a hole: the columns come from the manifest, every value is
/// bound, and nothing about the row's *keys* can introduce SQL. What it skips
/// is the required-field and computed-field checks, because the row already
/// passed those on its way out.
pub async fn insert_verbatim(
    db: &Db,
    schema: &str,
    model: &Model,
    row: &Map<String, Value>,
) -> Result<(), DataError> {
    let mut columns: Vec<String> = Vec::new();
    let mut holes: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();

    for (key, value) in row {
        if value.is_null() {
            continue;
        }
        // The declared spelling, or one of aichip's own three. Anything else is
        // skipped rather than refused: a bundle written by a newer aichip may
        // carry a column this one does not know, and dropping it loses one
        // field where failing loses the whole app.
        let (name, ty) = match model.field(key) {
            Some(f) => (f.name.clone(), f.ty.clone()),
            None if key == "id" => ("id".to_string(), FieldType::Ref(model.name.clone())),
            None if key == "created_at" || key == "updated_at" => {
                (key.clone(), FieldType::Datetime)
            }
            None => continue,
        };
        let Ok(text) = as_text(&ty, value) else {
            continue;
        };
        columns.push(query::quote(&name));
        holes.push(format!("${}::{}", params.len() + 1, query::cast(&ty)));
        params.push(text);
    }

    if columns.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "INSERT INTO {}.{} ({}) VALUES ({}) ON CONFLICT (id) DO NOTHING",
        query::quote(schema),
        query::quote(&model.name),
        columns.join(", "),
        holes.join(", ")
    );
    let mut q = sqlx::query(&sql);
    for param in &params {
        q = q.bind(param);
    }
    q.execute(&db.pool)
        .await
        .map_err(|e| DataError(format!("could not load a \"{}\" row: {e}", model.name)))?;
    Ok(())
}

pub async fn delete(db: &Db, schema: &str, model: &Model, id: Uuid) -> Result<bool, DataError> {
    let sql = format!(
        "DELETE FROM {}.{} WHERE \"id\" = $1::uuid",
        query::quote(schema),
        query::quote(&model.name)
    );
    let done = sqlx::query(&sql)
        .bind(id.to_string())
        .execute(&db.pool)
        .await
        .map_err(|e| DataError(format!("could not remove from \"{}\": {e}", model.name)))?;
    Ok(done.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::manifest;

    fn manifest_of(yaml: &str) -> Manifest {
        manifest::parse(yaml).expect("test manifest must parse")
    }

    fn model() -> Model {
        manifest_of(
            "name: T\nmodels:\n  expense:\n    fields:\n      \
             note:   { type: text, required: true }\n      \
             amount: { type: decimal }\n      \
             qty:    { type: int }\n      \
             total:  { type: decimal, compute: \"amount * qty\" }\n      \
             meta:   { type: json }\n",
        )
        .models
        .remove(0)
    }

    fn body(json: &str) -> Map<String, Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_key_the_model_does_not_have_is_refused_not_dropped() {
        // Silently ignoring it is how `amout` lives for a month.
        let e = writable(&model(), &body(r#"{"amout": 5}"#)).unwrap_err();
        assert!(e.0.contains("\"amout\""), "{e}");
    }

    #[test]
    fn the_columns_aichip_owns_cannot_be_written() {
        for reserved in RESERVED_FIELDS {
            let e = writable(&model(), &body(&format!("{{\"{reserved}\": \"x\"}}"))).unwrap_err();
            assert!(e.0.contains("set by aichip"), "{reserved}: {e}");
        }
    }

    #[test]
    fn a_computed_field_cannot_be_set_by_hand() {
        // It would be overwritten on the next save, so accepting it is a lie.
        let e = writable(&model(), &body(r#"{"total": 99}"#)).unwrap_err();
        assert!(e.0.contains("computed"), "{e}");
    }

    #[test]
    fn a_required_field_has_to_be_there_on_the_way_in() {
        let m = model();
        let values = writable(&m, &body(r#"{"qty": 2}"#)).unwrap();
        assert_eq!(missing_required(&m, &values), Some("note".into()));
        let values = writable(&m, &body(r#"{"note": "rent"}"#)).unwrap();
        assert_eq!(missing_required(&m, &values), None);
        // Explicitly null is the same as absent, and says so.
        assert!(writable(&m, &body(r#"{"note": null}"#))
            .unwrap_err()
            .0
            .contains("required"));
    }

    fn amount_of(json: &str) -> String {
        writable(&model(), &body(json))
            .unwrap()
            .into_iter()
            .find(|(n, _)| n == "amount")
            .unwrap()
            .1
            .expect("a decimal that was sent is never a clear")
    }

    #[test]
    fn a_decimal_sent_as_a_string_keeps_every_digit() {
        // The exact path, and the one the read projection round-trips into:
        // `numeric` comes back as text, so a row an app reads and writes back
        // is unchanged however many digits it has.
        assert_eq!(amount_of(r#"{"note":"x","amount": "10.15"}"#), "10.15");
        assert_eq!(
            amount_of(r#"{"note":"x","amount": "12345678901234567890.12"}"#),
            "12345678901234567890.12"
        );
        assert_eq!(amount_of(r#"{"note":"x","amount": "-0.01"}"#), "-0.01");
    }

    #[test]
    fn an_ordinary_decimal_sent_as_a_number_still_works() {
        // A JSON number is parsed into an f64 before this ever sees it.
        // Everyday values survive, because the shortest representation of that
        // double reads back the same — which is why sending money as a number
        // mostly looks fine, and why the string form exists for when it does
        // not.
        assert_eq!(amount_of(r#"{"note":"x","amount": 10.15}"#), "10.15");
        assert_eq!(amount_of(r#"{"note":"x","amount": 0}"#), "0");
        assert_eq!(amount_of(r#"{"note":"x","amount": -3.5}"#), "-3.5");
    }

    #[test]
    fn a_number_too_big_for_a_double_is_refused_and_says_what_to_do() {
        // It arrives as 1.2345678901234567e+19 — already rounded. Postgres
        // would happily store that, which is the quiet wrong answer: a ledger
        // that silently drops digits is worse than one that refuses.
        let e = writable(
            &model(),
            &body(r#"{"note":"x","amount": 12345678901234567890.12}"#),
        )
        .unwrap_err();
        assert!(e.0.contains("already rounded"), "{e}");
        assert!(e.0.contains("as a string"), "names the fix: {e}");
        // And the fix works.
        assert_eq!(
            amount_of(r#"{"note":"x","amount": "12345678901234567890.12"}"#),
            "12345678901234567890.12"
        );
    }

    #[test]
    fn something_that_is_not_a_number_is_refused_before_postgres_sees_it() {
        for bad_value in [
            r#""ten""#,
            r#""1.2.3""#,
            r#""NaN""#,
            r#""inf""#,
            r#""1e5""#,
            r#""""#,
        ] {
            let e = writable(
                &model(),
                &body(&format!(r#"{{"note":"x","amount": {bad_value}}}"#)),
            )
            .unwrap_err();
            assert!(
                e.0.contains("not a decimal"),
                "{bad_value} was accepted: {e}"
            );
        }
        assert!(writable(&model(), &body(r#"{"note":"x","qty": "many"}"#)).is_err());
    }

    #[test]
    fn json_fields_keep_their_shape() {
        let values = writable(&model(), &body(r#"{"note":"x","meta":{"a":[1,2]}}"#)).unwrap();
        let meta = values.iter().find(|(n, _)| n == "meta").unwrap();
        assert_eq!(meta.1.as_deref(), Some(r#"{"a":[1,2]}"#));
    }

    #[test]
    fn an_absent_optional_field_is_left_alone_rather_than_nulled() {
        // An update naming one field must not blank the others.
        let values = writable(&model(), &body(r#"{"qty": 3}"#)).unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].0, "qty");
    }

    #[test]
    fn an_optional_field_sent_as_null_is_cleared_rather_than_skipped() {
        // The bug: absent and explicitly null were the same thing, so this
        // dropped the field from the statement and the old value stayed on the
        // row — while the write reported success. "Mark them a no-show" left
        // the check-in time behind, so the record said they both did and did
        // not turn up.
        let values = writable(&model(), &body(r#"{"qty": null}"#)).unwrap();
        assert_eq!(values, vec![("qty".to_string(), None)]);

        // And the two really are different. Sending nothing about `qty` still
        // leaves it alone — an update naming one field must not blank the rest.
        assert!(writable(&model(), &body(r#"{"note": "x"}"#))
            .unwrap()
            .iter()
            .all(|(n, _)| n != "qty"));
    }

    #[test]
    fn a_required_field_still_cannot_be_cleared() {
        // `required` is not `NOT NULL` on the column (see schema.rs), so this
        // check is the only thing standing between a required field and an
        // empty one.
        let e = writable(&model(), &body(r#"{"note": null}"#)).unwrap_err();
        assert!(e.0.contains("required"), "{e}");
    }

    #[test]
    fn a_model_the_app_does_not_declare_lists_the_ones_it_does() {
        let m = manifest_of("name: T\nmodels:\n  expense: { fields: { a: { type: text } } }\n");
        let e = model_of(&m, "invoice").unwrap_err();
        assert!(e.0.contains("\"expense\""), "names what does exist: {e}");
    }
}
