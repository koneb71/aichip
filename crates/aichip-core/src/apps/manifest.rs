//! `aichip.app.yaml` — everything that defines an app, in one readable file.
//!
//! This is the whole format. A module declares models, views and actions and
//! aichip renders it; a container declares models and a port and serves its own
//! pages. Both are this file, which is what keeps them one feature.
//!
//! ## Why this is parsed by hand
//!
//! Three reasons, all about who writes manifests. An agent writes most of them
//! and a person reads every one.
//!
//! * **Order is meaning.** Fields appear on a form in the order they are
//!   declared. `#[derive(Deserialize)]` into a `BTreeMap` would alphabetise a
//!   form nobody asked to have alphabetised.
//! * **Errors have to name the thing.** `models.expense.fields.qty: unknown
//!   field type "intt"` is a fix; serde's default is a hunt.
//! * **Unknown keys are refused, not ignored.** An agent that writes `colums:`
//!   has written a view with no columns, and silently rendering an empty table
//!   is a worse outcome than refusing the manifest and saying which key.

use super::scope::Scope;
use serde_yaml::{Mapping, Value};
use std::fmt;

/// Columns aichip adds to every table, which a model therefore may not declare.
pub const RESERVED_FIELDS: [&str; 3] = ["id", "created_at", "updated_at"];

/// A problem with a manifest, and where in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    /// Dotted path to the offending key, e.g. `models.expense.fields.qty`.
    pub at: String,
    pub message: String,
}

impl ManifestError {
    fn new(at: impl Into<String>, message: impl Into<String>) -> Self {
        Self { at: at.into(), message: message.into() }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.at.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.at, self.message)
        }
    }
}

impl std::error::Error for ManifestError {}

type R<T> = Result<T, ManifestError>;

// ── The shapes ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    /// Declarative. aichip renders it and nothing arbitrary executes.
    Module,
    Node,
    Static,
}

impl Runtime {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Node => "node",
            Self::Static => "static",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "module" => Some(Self::Module),
            "node" => Some(Self::Node),
            "static" => Some(Self::Static),
            _ => None,
        }
    }
    /// Whether this runtime runs a container, and therefore carries the whole
    /// proxy, CSP and Dockerfile-approval apparatus.
    pub fn is_container(self) -> bool {
        !matches!(self, Self::Module)
    }
}

/// The closed set of field types.
///
/// Closed is what makes the generated DDL safe to build by string concatenation
/// and the export portable. Widening it is a deliberate act with a migration
/// attached, not something a manifest can do on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    Text,
    Int,
    Decimal,
    Bool,
    Date,
    Datetime,
    Json,
    /// A foreign key to another model *in the same app*.
    Ref(String),
}

impl FieldType {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "text" => Self::Text,
            "int" => Self::Int,
            "decimal" => Self::Decimal,
            "bool" => Self::Bool,
            "date" => Self::Date,
            "datetime" => Self::Datetime,
            "json" => Self::Json,
            other => Self::Ref(other.strip_prefix("ref:")?.to_string()),
        })
    }

    /// The Postgres type. Safe to interpolate: every arm is a literal.
    pub fn sql(&self) -> &'static str {
        match self {
            Self::Text => "TEXT",
            Self::Int => "BIGINT",
            Self::Decimal => "NUMERIC",
            Self::Bool => "BOOLEAN",
            Self::Date => "DATE",
            Self::Datetime => "TIMESTAMPTZ",
            Self::Json => "JSONB",
            Self::Ref(_) => "UUID",
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Ref(model) => format!("ref:{model}"),
            other => other.sql_name().to_string(),
        }
    }

    fn sql_name(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Int => "int",
            Self::Decimal => "decimal",
            Self::Bool => "bool",
            Self::Date => "date",
            Self::Datetime => "datetime",
            Self::Json => "json",
            Self::Ref(_) => "ref",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: FieldType,
    pub required: bool,
    /// An expression evaluated when a row is created and the field is absent.
    pub default: Option<String>,
    /// An expression evaluated on every write. A computed field is stored, not
    /// virtual, so it can be sorted and filtered like any other column.
    pub compute: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub name: String,
    pub fields: Vec<Field>,
    pub indexes: Vec<String>,
}

impl Model {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    List,
    Form,
    Kanban,
    Chart,
}

impl ViewKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Form => "form",
            Self::Kanban => "kanban",
            Self::Chart => "chart",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "list" => Some(Self::List),
            "form" => Some(Self::Form),
            "kanban" => Some(Self::Kanban),
            "chart" => Some(Self::Chart),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sort {
    pub field: String,
    pub descending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
}

impl ChartKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Line => "line",
            Self::Pie => "pie",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "bar" => Some(Self::Bar),
            "line" => Some(Self::Line),
            "pie" => Some(Self::Pie),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewSpec {
    List { columns: Vec<String>, sort: Option<Sort> },
    Form { groups: Vec<Vec<String>>, buttons: Vec<String> },
    Kanban { group_by: String, title: String, fields: Vec<String> },
    Chart { kind: ChartKind, group_by: String, measure: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub name: String,
    pub kind: ViewKind,
    pub model: String,
    pub spec: ViewSpec,
}

/// One step of an action. A closed set — this is the whole logic layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// `None` for a value means clear that field — see [`pairs`].
    Create { model: String, values: Vec<(String, Option<String>)> },
    Update { values: Vec<(String, Option<String>)> },
    Delete,
    CreateTask { project: Option<String>, title: String, prompt: String },
    StartRun { project: Option<String>, prompt: String, agent: Option<String> },
    Notify { message: String },
    Goto { view: String },
}

impl Step {
    /// What a person must have granted before this step may run.
    ///
    /// Per step rather than per action, so an action that mostly touches the
    /// app's own rows is not gated on the one step that reaches further — and
    /// so the refusal can name which step stopped.
    pub fn scope(&self) -> Option<Scope> {
        match self {
            Self::Create { .. } | Self::Update { .. } | Self::Delete => None,
            Self::Notify { .. } | Self::Goto { .. } => None,
            Self::CreateTask { .. } => Some(Scope::WriteBoard),
            Self::StartRun { .. } => Some(Scope::RunAgents),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Create { .. } => "create",
            Self::Update { .. } => "update",
            Self::Delete => "delete",
            Self::CreateTask { .. } => "create_task",
            Self::StartRun { .. } => "start_run",
            Self::Notify { .. } => "notify",
            Self::Goto { .. } => "goto",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub label: String,
    /// An expression. When it is false the button is not offered.
    pub show_if: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    /// For a module, the name of a declared view. For a container, the name of
    /// a screen — the file `views/<view>.html` the skeleton writes and the
    /// app's server routes.
    pub view: String,
    /// Container screens only: the model this screen is a CRUD page for, which
    /// is what selects the scaffold template. A module's views already name
    /// their model, so this stays `None` there.
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub icon: String,
    pub summary: String,
    pub runtime: Runtime,
    pub port: Option<u16>,
    pub scopes: Vec<Scope>,
    pub models: Vec<Model>,
    pub views: Vec<View>,
    pub actions: Vec<Action>,
    pub menu: Vec<MenuItem>,
}

impl Manifest {
    pub fn model(&self, name: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.name == name)
    }
    pub fn view(&self, name: &str) -> Option<&View> {
        self.views.iter().find(|v| v.name == name)
    }
    pub fn action(&self, name: &str) -> Option<&Action> {
        self.actions.iter().find(|a| a.name == name)
    }

    /// Every scope any action actually needs.
    ///
    /// Checked against `scopes` at parse time: an action that would need a
    /// permission the manifest never asked for is a manifest that would fail at
    /// the click instead of at the read.
    pub fn needed_scopes(&self) -> Vec<Scope> {
        let mut out: Vec<Scope> = self
            .actions
            .iter()
            .flat_map(|a| a.steps.iter().filter_map(Step::scope))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

// ── Parsing ─────────────────────────────────────────────────────────────────

/// Read and validate a manifest.
///
/// Everything is checked here, so anything downstream can treat a `Manifest` as
/// already coherent: types are known, refs point at real models, views name
/// real fields, and actions only need scopes the manifest asked for.
pub fn parse(yaml: &str) -> R<Manifest> {
    let root: Value = serde_yaml::from_str(yaml)
        .map_err(|e| ManifestError::new("", format!("this is not valid YAML: {e}")))?;
    let root = as_map(&root, "")?;
    known_keys(
        root,
        &[
            "name", "icon", "summary", "runtime", "port", "scopes", "models", "views", "actions",
            "menu",
        ],
        "",
    )?;

    let name = req_str(root, "name", "")?;
    if name.trim().is_empty() {
        return Err(ManifestError::new("name", "an app needs a name"));
    }
    let runtime_text = opt_str(root, "runtime", "")?.unwrap_or_else(|| "module".into());
    let runtime = Runtime::parse(&runtime_text).ok_or_else(|| {
        ManifestError::new(
            "runtime",
            format!("unknown runtime \"{runtime_text}\" — expected module, node or static"),
        )
    })?;

    let port = match root.get("port") {
        None => None,
        Some(v) => {
            let n = v.as_u64().filter(|n| (1..=65535).contains(n)).ok_or_else(|| {
                ManifestError::new("port", "a port must be a whole number from 1 to 65535")
            })?;
            Some(n as u16)
        }
    };

    let scopes = parse_scopes(root)?;
    let models = parse_models(root)?;

    // A container app draws its own pages, so `views:` and `actions:` have
    // nothing to attach to. Refused rather than accepted-and-ignored, which is
    // what happened before: the section parsed, nothing rendered it, and the
    // author was left staring at a working manifest and a blank app.
    if runtime.is_container() {
        for section in ["views", "actions"] {
            if root.get(section).is_some() {
                return Err(ManifestError::new(
                    section,
                    format!(
                        "a {} app draws its own pages — declare screens under \
                         menu:, not {section}:",
                        runtime.as_str()
                    ),
                ));
            }
        }
    }

    let views = parse_views(root, &models)?;
    let actions = parse_actions(root, &models, &views)?;
    let menu = parse_menu(root, runtime, &views, &models)?;

    let manifest = Manifest {
        name,
        icon: opt_str(root, "icon", "")?.unwrap_or_else(|| "▦".into()),
        summary: opt_str(root, "summary", "")?.unwrap_or_default(),
        runtime,
        port,
        scopes,
        models,
        views,
        actions,
        menu,
    };

    // Expressions are checked here rather than at the first write, for the same
    // reason as everything else in this function: a manifest that installs and
    // then fails the moment someone uses it has told them nothing useful.
    check_expressions(&manifest)?;

    // An action needing a permission the manifest never requested would fail at
    // the click, having looked fine at install. Catch it at the read instead.
    for needed in manifest.needed_scopes() {
        if !manifest.scopes.contains(&needed) {
            return Err(ManifestError::new(
                "scopes",
                format!(
                    "an action uses a step that needs \"{needed}\", but the manifest \
                     does not request that scope"
                ),
            ));
        }
    }

    Ok(manifest)
}

/// Every expression parses, and only names fields that exist.
///
/// A `compute` referring to a field nobody declared is the interesting case: it
/// evaluates to null rather than failing, so without this check the symptom is
/// a column that is silently always empty — the hardest kind of wrong to notice.
fn check_expressions(m: &Manifest) -> R<()> {
    let check = |src: &str, at: String, model: Option<&Model>| -> R<()> {
        let ast = super::expr::parse(src).map_err(|e| ManifestError::new(&at, e.0))?;
        if let Some(model) = model {
            for name in super::expr::fields_used(&ast) {
                if RESERVED_FIELDS.contains(&name.as_str()) || model.field(&name).is_some() {
                    continue;
                }
                return Err(ManifestError::new(
                    &at,
                    format!("\"{name}\" is not a field of \"{}\"", model.name),
                ));
            }
        }
        Ok(())
    };

    for model in &m.models {
        for field in &model.fields {
            let at = format!("models.{}.fields.{}", model.name, field.name);
            if let Some(src) = &field.compute {
                check(src, format!("{at}.compute"), Some(model))?;
            }
            if let Some(src) = &field.default {
                // A default is evaluated before the row exists, so it may not
                // read the row it is about to become part of.
                check(src, format!("{at}.default"), None)?;
            }
        }
    }

    for action in &m.actions {
        if let Some(src) = &action.show_if {
            check(src, format!("actions.{}.show_if", action.name), None)?;
        }
    }

    // A chart's measure is its own tiny grammar rather than a general
    // expression: `sum(total)` reduces a whole column, which is a different
    // thing from anything the expression language does to one record, and
    // conflating them would mean teaching it about aggregation.
    for view in &m.views {
        if let ViewSpec::Chart { measure, .. } = &view.spec {
            let model = m.model(&view.model).expect("views resolve their model at parse");
            parse_measure(measure)
                .and_then(|(_, field)| match field {
                    None => Ok(()),
                    Some(f) if RESERVED_FIELDS.contains(&f.as_str()) => Ok(()),
                    Some(f) if model.field(&f).is_some() => Ok(()),
                    Some(f) => Err(format!("\"{f}\" is not a field of \"{}\"", model.name)),
                })
                .map_err(|e| ManifestError::new(format!("views.{}.measure", view.name), e))?;
        }
    }

    Ok(())
}

/// Read `sum(total)`, `count()` and friends.
///
/// Returns the aggregate and the field it reduces — `None` for `count()`, which
/// counts rows rather than values.
pub fn parse_measure(src: &str) -> Result<(Agg, Option<String>), String> {
    let trimmed = src.trim();
    let Some((name, rest)) = trimmed.split_once('(') else {
        return Err(format!(
            "\"{trimmed}\" is not a measure — expected count(), sum(field), \
             avg(field), min(field) or max(field)"
        ));
    };
    let Some(inner) = rest.strip_suffix(')') else {
        return Err(format!("\"{trimmed}\" is missing its closing bracket"));
    };
    let agg = Agg::parse(name.trim())
        .ok_or_else(|| format!("\"{}\" is not one of count, sum, avg, min, max", name.trim()))?;
    let field = inner.trim();
    match (agg, field.is_empty()) {
        (Agg::Count, _) => Ok((agg, (!field.is_empty()).then(|| field.to_string()))),
        (_, true) => Err(format!("{} needs a field to add up", agg.as_str())),
        (_, false) => Ok((agg, Some(field.to_string()))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agg {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

impl Agg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "count" => Self::Count,
            "sum" => Self::Sum,
            "avg" => Self::Avg,
            "min" => Self::Min,
            "max" => Self::Max,
            _ => return None,
        })
    }
}

fn parse_scopes(root: &Mapping) -> R<Vec<Scope>> {
    let mut out = Vec::new();
    for (i, text) in opt_seq_str(root, "scopes", "")?.into_iter().enumerate() {
        let scope = Scope::parse(&text).ok_or_else(|| {
            ManifestError::new(format!("scopes[{i}]"), format!("unknown scope \"{text}\""))
        })?;
        if !out.contains(&scope) {
            out.push(scope);
        }
    }
    Ok(out)
}

fn parse_models(root: &Mapping) -> R<Vec<Model>> {
    let Some(models) = root.get("models") else {
        return Ok(Vec::new());
    };
    let models = as_map(models, "models")?;
    let mut out: Vec<Model> = Vec::new();

    for (key, body) in models {
        let name = key_name(key, "models")?;
        let at = format!("models.{name}");
        ident(&name, &at, "a model name")?;
        if out.iter().any(|m| m.name == name) {
            return Err(ManifestError::new(&at, "this model is declared twice"));
        }
        let body = as_map(body, &at)?;
        known_keys(body, &["fields", "indexes"], &at)?;

        let fields_at = format!("{at}.fields");
        let fields_map = body
            .get("fields")
            .ok_or_else(|| ManifestError::new(&at, "a model needs at least one field"))?;
        let fields_map = as_map(fields_map, &fields_at)?;
        let mut fields: Vec<Field> = Vec::new();

        for (fkey, fbody) in fields_map {
            let fname = key_name(fkey, &fields_at)?;
            let fat = format!("{fields_at}.{fname}");
            ident(&fname, &fat, "a field name")?;
            if RESERVED_FIELDS.contains(&fname.as_str()) {
                return Err(ManifestError::new(
                    &fat,
                    format!(
                        "\"{fname}\" is added to every table by aichip, so a model \
                         cannot declare it"
                    ),
                ));
            }
            if fields.iter().any(|f| f.name == fname) {
                return Err(ManifestError::new(&fat, "this field is declared twice"));
            }
            let fbody = as_map(fbody, &fat)?;
            known_keys(fbody, &["type", "required", "default", "compute", "label"], &fat)?;

            let ty_text = req_str(fbody, "type", &fat)?;
            let ty = FieldType::parse(&ty_text).ok_or_else(|| {
                ManifestError::new(
                    format!("{fat}.type"),
                    format!(
                        "unknown field type \"{ty_text}\" — expected one of \
                         text, int, decimal, bool, date, datetime, json, or ref:<model>"
                    ),
                )
            })?;
            let compute = opt_str(fbody, "compute", &fat)?;
            let default = opt_str(fbody, "default", &fat)?;
            if compute.is_some() && default.is_some() {
                return Err(ManifestError::new(
                    &fat,
                    "a field cannot have both a default and a compute — a computed \
                     field is written on every save, so the default could never be seen",
                ));
            }

            fields.push(Field {
                name: fname,
                ty,
                required: opt_bool(fbody, "required", &fat)?.unwrap_or(false),
                default,
                compute,
                label: opt_str(fbody, "label", &fat)?,
            });
        }

        if fields.is_empty() {
            return Err(ManifestError::new(&at, "a model needs at least one field"));
        }

        let mut indexes = Vec::new();
        for (i, field) in opt_seq_str(body, "indexes", &at)?.into_iter().enumerate() {
            if !fields.iter().any(|f| f.name == field) {
                return Err(ManifestError::new(
                    format!("{at}.indexes[{i}]"),
                    format!("\"{field}\" is not a field of this model"),
                ));
            }
            indexes.push(field);
        }

        out.push(Model { name, fields, indexes });
    }

    // Refs are resolved after every model is known, so order of declaration
    // does not decide whether a manifest is legal.
    for model in &out {
        for field in &model.fields {
            if let FieldType::Ref(target) = &field.ty {
                if !out.iter().any(|m| &m.name == target) {
                    return Err(ManifestError::new(
                        format!("models.{}.fields.{}.type", model.name, field.name),
                        format!("ref:{target} points at a model this app does not declare"),
                    ));
                }
            }
        }
    }

    Ok(out)
}

fn parse_views(root: &Mapping, models: &[Model]) -> R<Vec<View>> {
    let Some(views) = root.get("views") else {
        return Ok(Vec::new());
    };
    let views = as_map(views, "views")?;
    let mut out: Vec<View> = Vec::new();

    for (key, body) in views {
        let name = key_name(key, "views")?;
        let at = format!("views.{name}");
        ident(&name, &at, "a view name")?;
        if out.iter().any(|v| v.name == name) {
            return Err(ManifestError::new(&at, "this view is declared twice"));
        }
        let body = as_map(body, &at)?;
        known_keys(
            body,
            &[
                "kind", "shape", "model", "columns", "sort", "groups", "fields", "buttons",
                "group_by", "title", "measure",
            ],
            &at,
        )?;

        // The kind may be left off when the view is *named* for it, which is
        // what makes the one-model common case read as plainly as the plan's
        // example. Anything else has to say.
        //
        // The name wins when it is one of the four, because `kind` does double
        // duty: on a view named `chart`, `kind: bar` is the chart's shape, not
        // a second opinion about what kind of view this is. Two answers that
        // genuinely disagree are refused rather than silently resolved.
        let kind_text = opt_str(body, "kind", &at)?;
        let kind = match ViewKind::parse(&name) {
            Some(from_name) => {
                if let Some(text) = &kind_text {
                    if let Some(from_key) = ViewKind::parse(text) {
                        if from_key != from_name {
                            return Err(ManifestError::new(
                                format!("{at}.kind"),
                                format!(
                                    "this view is named \"{name}\" but says it is a \
                                     \"{text}\" — rename it or drop the `kind`"
                                ),
                            ));
                        }
                    }
                }
                from_name
            }
            None => {
                let text = kind_text.clone().ok_or_else(|| {
                    ManifestError::new(
                        &at,
                        "this view needs a `kind` — only a view named list, form, kanban \
                         or chart may leave it out",
                    )
                })?;
                ViewKind::parse(&text).ok_or_else(|| {
                    ManifestError::new(
                        format!("{at}.kind"),
                        format!(
                            "unknown view kind \"{text}\" — expected list, form, kanban or chart"
                        ),
                    )
                })?
            }
        };

        // Likewise the model, when the app declares exactly one.
        let model_name = match opt_str(body, "model", &at)? {
            Some(m) => m,
            None => match models {
                [only] => only.name.clone(),
                [] => return Err(ManifestError::new(&at, "this app declares no models")),
                _ => {
                    return Err(ManifestError::new(
                        &at,
                        "this view needs a `model` — it can only be left out when the \
                         app declares exactly one",
                    ))
                }
            },
        };
        let model = models.iter().find(|m| m.name == model_name).ok_or_else(|| {
            ManifestError::new(
                format!("{at}.model"),
                format!("\"{model_name}\" is not a model this app declares"),
            )
        })?;

        let spec = parse_view_spec(kind, body, model, &at)?;
        out.push(View { name, kind, model: model_name, spec });
    }

    Ok(out)
}

fn parse_view_spec(kind: ViewKind, body: &Mapping, model: &Model, at: &str) -> R<ViewSpec> {
    // Every field a view names has to exist, or the first thing a person sees
    // after installing is a blank column with no explanation.
    let check = |field: &str, where_: &str| -> R<()> {
        if RESERVED_FIELDS.contains(&field) || model.field(field).is_some() {
            Ok(())
        } else {
            Err(ManifestError::new(
                where_,
                format!("\"{field}\" is not a field of model \"{}\"", model.name),
            ))
        }
    };

    Ok(match kind {
        ViewKind::List => {
            let columns = opt_seq_str(body, "columns", at)?;
            let columns = if columns.is_empty() {
                model.fields.iter().map(|f| f.name.clone()).collect()
            } else {
                columns
            };
            for (i, c) in columns.iter().enumerate() {
                check(c, &format!("{at}.columns[{i}]"))?;
            }
            let sort = match opt_str(body, "sort", at)? {
                None => None,
                Some(text) => {
                    let (descending, field) = match text.strip_prefix('-') {
                        Some(rest) => (true, rest.to_string()),
                        None => (false, text.clone()),
                    };
                    check(&field, &format!("{at}.sort"))?;
                    Some(Sort { field, descending })
                }
            };
            ViewSpec::List { columns, sort }
        }
        ViewKind::Form => {
            let mut groups: Vec<Vec<String>> = Vec::new();
            if let Some(v) = body.get("groups") {
                let seq = v.as_sequence().ok_or_else(|| {
                    ManifestError::new(format!("{at}.groups"), "expected a list of lists of fields")
                })?;
                for (i, group) in seq.iter().enumerate() {
                    let gat = format!("{at}.groups[{i}]");
                    let inner = group.as_sequence().ok_or_else(|| {
                        ManifestError::new(&gat, "expected a list of field names")
                    })?;
                    let mut fields = Vec::new();
                    for (j, f) in inner.iter().enumerate() {
                        let f = f.as_str().ok_or_else(|| {
                            ManifestError::new(format!("{gat}[{j}]"), "expected a field name")
                        })?;
                        check(f, &format!("{gat}[{j}]"))?;
                        fields.push(f.to_string());
                    }
                    groups.push(fields);
                }
            }
            // `fields:` is the flat spelling of a form with one group.
            let flat = opt_seq_str(body, "fields", at)?;
            if !flat.is_empty() {
                for (i, f) in flat.iter().enumerate() {
                    check(f, &format!("{at}.fields[{i}]"))?;
                }
                groups.push(flat);
            }
            if groups.is_empty() {
                groups.push(model.fields.iter().map(|f| f.name.clone()).collect());
            }
            ViewSpec::Form { groups, buttons: opt_seq_str(body, "buttons", at)? }
        }
        ViewKind::Kanban => {
            let group_by = req_str(body, "group_by", at)?;
            check(&group_by, &format!("{at}.group_by"))?;
            let title = match opt_str(body, "title", at)? {
                Some(t) => {
                    check(&t, &format!("{at}.title"))?;
                    t
                }
                None => model.fields[0].name.clone(),
            };
            let fields = opt_seq_str(body, "fields", at)?;
            for (i, f) in fields.iter().enumerate() {
                check(f, &format!("{at}.fields[{i}]"))?;
            }
            ViewSpec::Kanban { group_by, title, fields }
        }
        ViewKind::Chart => {
            // `shape` is the unambiguous spelling and the only one that works
            // for a chart view *not* named "chart". `kind: bar` is accepted
            // because that is what the documented example writes, and on a view
            // named "chart" the key is free to mean the shape — but `kind:
            // chart` carries no shape in it, so it falls through to the default
            // rather than being read as a chart type.
            let shape_text = opt_str(body, "shape", at)?
                .or_else(|| opt_str(body, "kind", at).ok().flatten().filter(|t| t != "chart"))
                .unwrap_or_else(|| "bar".into());
            let chart = ChartKind::parse(&shape_text).ok_or_else(|| {
                ManifestError::new(
                    format!("{at}.shape"),
                    format!("unknown chart shape \"{shape_text}\" — expected bar, line or pie"),
                )
            })?;
            let group_by = req_str(body, "group_by", at)?;
            check(&group_by, &format!("{at}.group_by"))?;
            ViewSpec::Chart { kind: chart, group_by, measure: req_str(body, "measure", at)? }
        }
    })
}

fn parse_actions(root: &Mapping, models: &[Model], views: &[View]) -> R<Vec<Action>> {
    let Some(actions) = root.get("actions") else {
        return Ok(Vec::new());
    };
    let actions = as_map(actions, "actions")?;
    let mut out: Vec<Action> = Vec::new();

    for (key, body) in actions {
        let name = key_name(key, "actions")?;
        let at = format!("actions.{name}");
        ident(&name, &at, "an action name")?;
        if out.iter().any(|a| a.name == name) {
            return Err(ManifestError::new(&at, "this action is declared twice"));
        }
        let body = as_map(body, &at)?;
        known_keys(body, &["label", "show_if", "steps"], &at)?;

        let steps_at = format!("{at}.steps");
        let steps_seq = body
            .get("steps")
            .and_then(Value::as_sequence)
            .ok_or_else(|| ManifestError::new(&steps_at, "an action needs a list of steps"))?;
        if steps_seq.is_empty() {
            return Err(ManifestError::new(&steps_at, "an action needs at least one step"));
        }

        let mut steps = Vec::new();
        for (i, step) in steps_seq.iter().enumerate() {
            steps.push(parse_step(step, models, views, &format!("{steps_at}[{i}]"))?);
        }

        out.push(Action {
            name: name.clone(),
            label: opt_str(body, "label", &at)?.unwrap_or(name),
            show_if: opt_str(body, "show_if", &at)?,
            steps,
        });
    }

    Ok(out)
}

fn parse_step(step: &Value, models: &[Model], views: &[View], at: &str) -> R<Step> {
    let map = as_map(step, at)?;
    // One key: the step's kind. Two would be two steps pretending to be one,
    // and the order they ran in would be whatever the YAML parser felt like.
    if map.len() != 1 {
        return Err(ManifestError::new(
            at,
            "a step is exactly one of create, update, delete, create_task, \
             start_run, notify or goto",
        ));
    }
    let (key, body) = map.iter().next().expect("length was just checked");
    let kind = key_name(key, at)?;
    let at = &format!("{at}.{kind}");

    Ok(match kind.as_str() {
        "delete" => Step::Delete,
        "create" => {
            let body = as_map(body, at)?;
            known_keys(body, &["model", "values"], at)?;
            let model = req_str(body, "model", at)?;
            let model_ref = models.iter().find(|m| m.name == model).ok_or_else(|| {
                ManifestError::new(
                    format!("{at}.model"),
                    format!("\"{model}\" is not a model this app declares"),
                )
            })?;
            Step::Create { values: parse_values(body, model_ref, at)?, model }
        }
        "update" => {
            // The map *is* the values — `update: { category: pending }` — with
            // no `values:` wrapper, because an update has nothing else to say
            // and the wrapper would be a level of nesting that carries no
            // information. `create` keeps one only because it must also name a
            // model.
            //
            // Which model this updates is decided by the record the button sits
            // on, so the field names cannot be checked here. They are checked
            // against that model at the click.
            let values = pairs(as_map(body, at)?, at)?;
            if values.is_empty() {
                return Err(ManifestError::new(at, "an update step needs values to set"));
            }
            Step::Update { values }
        }
        "create_task" => {
            let body = as_map(body, at)?;
            known_keys(body, &["project", "title", "prompt"], at)?;
            Step::CreateTask {
                project: opt_str(body, "project", at)?,
                title: req_str(body, "title", at)?,
                prompt: req_str(body, "prompt", at)?,
            }
        }
        "start_run" => {
            let body = as_map(body, at)?;
            known_keys(body, &["project", "prompt", "agent"], at)?;
            Step::StartRun {
                project: opt_str(body, "project", at)?,
                prompt: req_str(body, "prompt", at)?,
                agent: opt_str(body, "agent", at)?,
            }
        }
        "notify" => Step::Notify {
            message: body
                .as_str()
                .map(str::to_string)
                .or_else(|| as_map(body, at).ok().and_then(|m| opt_str(m, "message", at).ok()?))
                .ok_or_else(|| ManifestError::new(at, "a notify step needs a message"))?,
        },
        "goto" => {
            let view = body
                .as_str()
                .map(str::to_string)
                .or_else(|| as_map(body, at).ok().and_then(|m| opt_str(m, "view", at).ok()?))
                .ok_or_else(|| ManifestError::new(at, "a goto step needs a view"))?;
            if !views.iter().any(|v| v.name == view) {
                return Err(ManifestError::new(
                    at,
                    format!("\"{view}\" is not a view this app declares"),
                ));
            }
            Step::Goto { view }
        }
        other => {
            return Err(ManifestError::new(
                at,
                format!(
                    "unknown step \"{other}\" — expected create, update, delete, \
                     create_task, start_run, notify or goto"
                ),
            ))
        }
    })
}

fn parse_values(body: &Mapping, model: &Model, at: &str) -> R<Vec<(String, Option<String>)>> {
    let vat = format!("{at}.values");
    let Some(values) = body.get("values") else {
        return Ok(Vec::new());
    };
    let values = pairs(as_map(values, &vat)?, &vat)?;
    for (field, value) in &values {
        let Some(declared) = model.field(field) else {
            return Err(ManifestError::new(
                format!("{vat}.{field}"),
                format!("\"{field}\" is not a field of model \"{}\"", model.name),
            ));
        };
        // Caught here rather than at the click, because a create names its
        // model in the manifest — so this is knowable while somebody is still
        // looking at the manifest, and a row that could never be written should
        // not install.
        if value.is_none() && declared.required {
            return Err(ManifestError::new(
                format!("{vat}.{field}"),
                format!(
                    "\"{field}\" is required, so it cannot be created empty — \
                     give it a value, or drop `required` from the field"
                ),
            ));
        }
    }
    Ok(values)
}

/// The menu means something different per runtime, and the parser says which.
///
/// For a **module** an entry points at a declared view, exactly as before. For
/// a **container** an entry declares a *screen*: `view` names the HTML file
/// the skeleton writes (`views/<view>.html`) and the path the app's server
/// routes, so it goes through [`ident`] — it becomes a filename and a URL, and
/// the charset is the defence, same as everywhere else identifiers travel.
/// An optional `model:` binds the screen to a declared model, which is what
/// selects the CRUD template at scaffold time.
///
/// Screens are declared here rather than inferred from the `views/` directory
/// because the sidebar and the tab bar read the manifest — a screen that only
/// exists as a file would be reachable but invisible.
fn parse_menu(
    root: &Mapping,
    runtime: Runtime,
    views: &[View],
    models: &[Model],
) -> R<Vec<MenuItem>> {
    let Some(menu) = root.get("menu") else {
        return Ok(Vec::new());
    };
    let seq = menu
        .as_sequence()
        .ok_or_else(|| ManifestError::new("menu", "expected a list of menu entries"))?;
    let mut out = Vec::new();
    for (i, item) in seq.iter().enumerate() {
        let at = format!("menu[{i}]");
        let map = as_map(item, &at)?;

        if runtime.is_container() {
            known_keys(map, &["label", "view", "model"], &at)?;
            let view = req_str(map, "view", &at)?;
            ident(&view, &format!("{at}.view"), "a screen name")?;
            let model = opt_str(map, "model", &at)?;
            if let Some(m) = &model {
                if !models.iter().any(|declared| &declared.name == m) {
                    return Err(ManifestError::new(
                        format!("{at}.model"),
                        format!("\"{m}\" is not a model this app declares"),
                    ));
                }
            }
            out.push(MenuItem {
                label: opt_str(map, "label", &at)?.unwrap_or_else(|| view.clone()),
                view,
                model,
            });
        } else {
            known_keys(map, &["label", "view"], &at)?;
            let view = req_str(map, "view", &at)?;
            let target = views.iter().find(|v| v.name == view).ok_or_else(|| {
                ManifestError::new(
                    format!("{at}.view"),
                    format!("\"{view}\" is not a view this app declares"),
                )
            })?;
            out.push(MenuItem {
                label: opt_str(map, "label", &at)?.unwrap_or_else(|| target.name.clone()),
                view,
                model: None,
            });
        }
    }
    Ok(out)
}

// ── Walking YAML ────────────────────────────────────────────────────────────

fn as_map<'a>(v: &'a Value, at: &str) -> R<&'a Mapping> {
    v.as_mapping()
        .ok_or_else(|| ManifestError::new(at, "expected a block of keys here"))
}

fn key_name(key: &Value, at: &str) -> R<String> {
    key.as_str()
        .map(str::to_string)
        .ok_or_else(|| ManifestError::new(at, "expected a name here"))
}

/// Refuse keys the format does not define.
///
/// The failure this prevents is quiet: an agent writes `colums:` and gets a
/// table with no columns and no complaint. A misspelling has to be an error,
/// because the alternative is a manifest that looks fine and renders wrong.
fn known_keys(map: &Mapping, allowed: &[&str], at: &str) -> R<()> {
    for key in map.keys() {
        let name = key_name(key, at)?;
        if !allowed.contains(&name.as_str()) {
            return Err(ManifestError::new(
                if at.is_empty() { name.clone() } else { format!("{at}.{name}") },
                format!("unknown key \"{name}\" — expected one of {}", allowed.join(", ")),
            ));
        }
    }
    Ok(())
}

fn req_str(map: &Mapping, key: &str, at: &str) -> R<String> {
    opt_str(map, key, at)?.ok_or_else(|| {
        ManifestError::new(
            if at.is_empty() { key.to_string() } else { format!("{at}.{key}") },
            "this key is required",
        )
    })
}

fn opt_str(map: &Mapping, key: &str, at: &str) -> R<Option<String>> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        // Numbers and booleans are accepted where text is wanted: `default: 1`
        // is what a person writes for an int field, and refusing it would be
        // pedantry about YAML rather than about the manifest.
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(Value::Bool(b)) => Ok(Some(b.to_string())),
        Some(_) => Err(ManifestError::new(
            if at.is_empty() { key.to_string() } else { format!("{at}.{key}") },
            "expected a single value here",
        )),
    }
}

fn opt_bool(map: &Mapping, key: &str, at: &str) -> R<Option<bool>> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(ManifestError::new(
            format!("{at}.{key}"),
            "expected true or false",
        )),
    }
}

fn opt_seq_str(map: &Mapping, key: &str, at: &str) -> R<Vec<String>> {
    let path = if at.is_empty() { key.to_string() } else { format!("{at}.{key}") };
    match map.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Sequence(items)) => items
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| ManifestError::new(format!("{path}[{i}]"), "expected a name"))
            })
            .collect(),
        Some(_) => Err(ManifestError::new(path, "expected a list")),
    }
}

/// The `field: value` pairs of a create or update step.
///
/// A key written with nothing after it — `checked_in_at:` — means **clear this
/// field**, and `None` is how that travels. It is the only way to say it:
/// values are templates rather than expressions, so there is no text a person
/// could type that arrives as SQL NULL instead of as the characters they typed.
///
/// Refusing it, which is what this did, rejected a manifest whose keys were all
/// spelled correctly — and the action that wants it is the ordinary one.
/// Marking somebody a no-show has to take the check-in time back off the row,
/// or the record says they both did and did not turn up.
///
/// Whether the field *may* be cleared is not decided here. For a create the
/// model is known and [`parse_values`] refuses clearing a required field; for
/// an update it is decided by the record the button sits on, so `data::writable`
/// refuses it at the click, where the model is finally known.
fn pairs(map: &Mapping, at: &str) -> R<Vec<(String, Option<String>)>> {
    let mut out = Vec::new();
    for (k, _) in map {
        let name = key_name(k, at)?;
        // `opt_str` already refuses a sequence or a map here, and already reads
        // a bare key as `None` — the distinction this needed all along.
        let value = opt_str(map, &name, at)?;
        out.push((name, value));
    }
    Ok(out)
}

/// A name safe to put in generated SQL, and readable when it comes back out.
///
/// Strict on purpose. Every identifier from a manifest is interpolated into DDL
/// and into queries, and quoting alone is a defence that depends on getting the
/// quoting right everywhere forever. A charset this narrow means there is
/// nothing to escape in the first place.
fn ident(name: &str, at: &str, what: &str) -> R<()> {
    let ok = !name.is_empty()
        && name.len() <= 48
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(ManifestError::new(
            at,
            format!(
                "{what} must be lower-case letters, digits and underscores, start \
                 with a letter, and be at most 48 characters — \"{name}\" is not"
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manifest from the plan, which is also what the scaffold prompt shows
    /// an agent. If this stops parsing, the documented example is a lie.
    const EXAMPLE: &str = r#"
name: Expenses
icon: "▤"
summary: Track spending and ask an agent to categorise it
runtime: module
scopes: [read:projects, run:agents]

models:
  expense:
    fields:
      description: { type: text, required: true }
      amount:      { type: decimal }
      qty:         { type: int, default: 1 }
      total:       { type: decimal, compute: "amount * qty" }
      spent_on:    { type: date, default: "today()" }
      category:    { type: text }
    indexes: [spent_on]

views:
  list:
    columns: [spent_on, description, category, total]
    sort: "-spent_on"
  form:
    groups:
      - [description, amount, qty, total]
      - [spent_on, category]
    buttons: [categorise]
  chart:
    kind: bar
    group_by: category
    measure: "sum(total)"

actions:
  categorise:
    label: Ask an agent to categorise
    show_if: "category == ''"
    steps:
      - start_run:
          prompt: "Categorise this expense: {{ record.description }}"
      - update: { category: "pending" }

menu:
  - { label: Expenses, view: list }
  - { label: By category, view: chart }
"#;

    fn err(yaml: &str) -> ManifestError {
        parse(yaml).expect_err("expected this manifest to be refused")
    }

    #[test]
    fn the_documented_example_parses() {
        let m = parse(EXAMPLE).expect("the example in the plan must parse");
        assert_eq!(m.name, "Expenses");
        assert_eq!(m.runtime, Runtime::Module);
        assert_eq!(m.models.len(), 1);
        assert_eq!(m.views.len(), 3);
        assert_eq!(m.menu.len(), 2);
        assert_eq!(m.scopes, vec![Scope::ReadProjects, Scope::RunAgents]);
    }

    // ── Container menus: screens, not views ─────────────────────────────────

    const CONTAINER: &str = "\
name: Tracker\nruntime: node\n\
models:\n  task:\n    fields:\n      title: { type: text, required: true }\n";

    #[test]
    fn a_container_menu_declares_screens_with_an_optional_model() {
        let m = parse(&format!(
            "{CONTAINER}menu:\n  - {{ label: Tasks, view: tasks, model: task }}\n  - {{ view: about }}\n"
        ))
        .expect("a container app with screens must parse");
        assert_eq!(m.menu.len(), 2);
        assert_eq!(m.menu[0].view, "tasks");
        assert_eq!(m.menu[0].model.as_deref(), Some("task"));
        // No label falls back to the screen name; no model means a stub page.
        assert_eq!(m.menu[1].label, "about");
        assert_eq!(m.menu[1].model, None);
    }

    #[test]
    fn a_screen_bound_to_a_model_the_app_does_not_declare_is_refused() {
        let e = err(&format!("{CONTAINER}menu:\n  - {{ view: notes, model: note }}\n"));
        assert_eq!(e.at, "menu[0].model");
        assert!(e.message.contains("not a model"), "{}", e.message);
    }

    #[test]
    fn a_screen_name_becomes_a_filename_so_it_keeps_the_ident_charset() {
        // `view` is written to disk as views/<view>.html and routed as a URL
        // path. The charset is the defence — nothing to escape anywhere.
        let e = err(&format!("{CONTAINER}menu:\n  - {{ view: \"../etc\" }}\n"));
        assert_eq!(e.at, "menu[0].view");
    }

    #[test]
    fn a_module_menu_does_not_take_a_model_key() {
        // A module view already names its model; a second binding here could
        // only agree or contradict it.
        let e = err(
            "name: T\nmodels:\n  a: { fields: { x: { type: text } } }\n\
             views:\n  list: { model: a }\n\
             menu:\n  - { view: list, model: a }\n",
        );
        assert!(e.at.starts_with("menu[0]"), "{}", e.at);
    }

    #[test]
    fn a_container_app_declaring_views_is_refused_not_ignored() {
        // Before this, the section parsed and nothing rendered it — a working
        // manifest and a blank app, with no hint which line was dead weight.
        let e = err(&format!("{CONTAINER}views:\n  list: {{ model: task }}\n"));
        assert_eq!(e.at, "views");
        assert!(e.message.contains("menu:"), "points at the fix: {}", e.message);

        let e = err(&format!(
            "{CONTAINER}actions:\n  go:\n    label: Go\n    steps: [ delete ]\n"
        ));
        assert_eq!(e.at, "actions");
    }

    /// The manifest the "Write it for me" button produced for an event
    /// attendance app, trimmed to the action that would not install.
    ///
    /// It failed on `checked_in_at:` with "expected a value" — a key spelled
    /// exactly right, refused for saying the one thing an update of this kind
    /// has to say. Marking somebody a no-show has to take the check-in time
    /// back off the row, or the record says they both did and did not turn up.
    #[test]
    fn a_key_with_nothing_after_it_clears_that_field() {
        let m = parse(
            "name: Attendance\nruntime: module\n\
             models:\n  attendee:\n    fields:\n      \
             name: { type: text, required: true }\n      \
             status: { type: text }\n      \
             checked_in_at: { type: datetime }\n\
             views:\n  list: { model: attendee }\n\
             actions:\n  mark_no_show:\n    label: No show\n    steps:\n      \
             - update: { status: \"no_show\", checked_in_at: }\n",
        )
        .expect("a manifest that clears a field must install");

        let Step::Update { values } = &m.actions[0].steps[0] else {
            panic!("expected an update step");
        };
        assert_eq!(
            values,
            &vec![
                ("status".to_string(), Some("no_show".to_string())),
                // The distinction the whole change exists for: `None` is
                // "clear it", not "the author forgot".
                ("checked_in_at".to_string(), None),
            ]
        );
    }

    #[test]
    fn creating_a_row_with_a_required_field_left_empty_is_refused() {
        // A create names its model, so this is knowable while somebody is
        // still looking at the manifest — rather than at the click, on a row
        // that could never have been written.
        let e = err(
            "name: T\nruntime: module\n\
             models:\n  note:\n    fields:\n      \
             title: { type: text, required: true }\n\
             views:\n  list: { model: note }\n\
             actions:\n  blank:\n    label: Blank\n    steps:\n      \
             - create: { model: note, values: { title: } }\n",
        );
        assert!(e.at.ends_with("title"), "names the field: {}", e.at);
        assert!(e.message.contains("required"), "{}", e.message);
    }

    #[test]
    fn an_update_step_with_no_fields_at_all_is_still_refused() {
        // Clearing a field is a write; clearing *nothing* is a step that does
        // nothing, and stays an error.
        let e = err(
            "name: T\nruntime: module\n\
             models:\n  note: { fields: { a: { type: text } } }\n\
             views:\n  list: { model: note }\n\
             actions:\n  nothing:\n    label: Nothing\n    steps:\n      \
             - update: {}\n",
        );
        assert!(e.message.contains("needs values"), "{}", e.message);
    }

    #[test]
    fn fields_keep_the_order_they_were_declared_in() {
        // A form renders in declaration order. Sorting them — which is what a
        // BTreeMap would have done for free — silently rearranges every form.
        let m = parse(EXAMPLE).unwrap();
        let names: Vec<&str> = m.models[0].fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["description", "amount", "qty", "total", "spent_on", "category"]
        );
    }

    #[test]
    fn a_view_named_for_its_kind_needs_no_kind_and_one_model_needs_no_model() {
        let m = parse(EXAMPLE).unwrap();
        let list = m.view("list").unwrap();
        assert_eq!(list.kind, ViewKind::List);
        assert_eq!(list.model, "expense");
        match &list.spec {
            ViewSpec::List { sort, .. } => {
                let sort = sort.as_ref().unwrap();
                assert_eq!(sort.field, "spent_on");
                assert!(sort.descending, "a leading dash means descending");
            }
            other => panic!("expected a list view, got {other:?}"),
        }
    }

    #[test]
    fn a_view_must_say_which_model_when_there_is_more_than_one() {
        let e = err(r#"
name: Two
models:
  a: { fields: { x: { type: text } } }
  b: { fields: { y: { type: text } } }
views:
  list: { columns: [x] }
"#);
        assert_eq!(e.at, "views.list");
        assert!(e.message.contains("needs a `model`"), "{e}");
    }

    #[test]
    fn an_unknown_field_type_is_named_in_the_error() {
        let e = err(r#"
name: T
models:
  thing: { fields: { qty: { type: intt } } }
"#);
        assert_eq!(e.at, "models.thing.fields.qty.type");
        assert!(e.message.contains("\"intt\""), "{e}");
    }

    #[test]
    fn a_misspelled_key_is_refused_rather_than_ignored() {
        // The quiet failure this exists to prevent: `colums` parses fine as
        // "no columns", and the person gets an empty table with no explanation.
        let e = err(r#"
name: T
models:
  thing: { fields: { x: { type: text } } }
views:
  list: { colums: [x] }
"#);
        assert_eq!(e.at, "views.list.colums");
        assert!(e.message.contains("unknown key"), "{e}");
    }

    #[test]
    fn a_view_cannot_name_a_field_that_does_not_exist() {
        let e = err(r#"
name: T
models:
  thing: { fields: { x: { type: text } } }
views:
  list: { columns: [x, nope] }
"#);
        assert_eq!(e.at, "views.list.columns[1]");
        assert!(e.message.contains("\"nope\""), "{e}");
    }

    #[test]
    fn the_columns_aichip_adds_cannot_be_redeclared() {
        for reserved in RESERVED_FIELDS {
            let e = err(&format!(
                "name: T\nmodels:\n  thing:\n    fields:\n      {reserved}: {{ type: text }}\n"
            ));
            assert!(e.message.contains(reserved), "{e}");
        }
        // But a view may still show them — they are real columns.
        let m = parse(
            "name: T\nmodels:\n  thing: { fields: { x: { type: text } } }\n\
             views:\n  list: { columns: [created_at, x] }\n",
        )
        .unwrap();
        assert!(matches!(m.views[0].spec, ViewSpec::List { .. }));
    }

    #[test]
    fn a_ref_must_point_at_a_model_the_app_declares() {
        let e = err(r#"
name: T
models:
  line: { fields: { order_id: { type: "ref:order" } } }
"#);
        assert!(e.message.contains("ref:order"), "{e}");

        // Forward references are fine: refs resolve once every model is known,
        // so declaration order does not decide legality.
        let m = parse(r#"
name: T
models:
  line: { fields: { order_id: { type: "ref:order" } } }
  order: { fields: { total: { type: decimal } } }
"#)
        .unwrap();
        assert_eq!(m.models[0].fields[0].ty, FieldType::Ref("order".into()));
    }

    #[test]
    fn identifiers_are_narrow_enough_that_nothing_needs_escaping() {
        // These names are interpolated into DDL. The defence is the charset,
        // not the quoting.
        for bad in ["Thing", "th-ing", "1thing", "thing;drop", "th ing", "thing\"x"] {
            let e = err(&format!(
                "name: T\nmodels:\n  {bad}: {{ fields: {{ x: {{ type: text }} }} }}\n"
            ));
            assert!(e.message.contains("lower-case"), "{bad} was accepted: {e}");
        }
    }

    #[test]
    fn an_action_cannot_need_a_scope_the_manifest_never_asked_for() {
        // Otherwise it looks fine at install and fails at the click.
        let e = err(r#"
name: T
models:
  thing: { fields: { x: { type: text } } }
actions:
  go:
    steps:
      - start_run: { prompt: "hello" }
"#);
        assert_eq!(e.at, "scopes");
        assert!(e.message.contains("run:agents"), "{e}");
    }

    #[test]
    fn starting_a_run_and_filing_a_card_are_separately_gated() {
        let m = parse(r#"
name: T
scopes: [write:board, run:agents]
models:
  thing: { fields: { x: { type: text } } }
actions:
  file:
    steps:
      - create_task: { title: "t", prompt: "p" }
  spend:
    steps:
      - start_run: { prompt: "p" }
"#)
        .unwrap();
        assert_eq!(m.action("file").unwrap().steps[0].scope(), Some(Scope::WriteBoard));
        assert_eq!(m.action("spend").unwrap().steps[0].scope(), Some(Scope::RunAgents));
        // The app's own rows need nothing.
        assert_eq!(Step::Delete.scope(), None);
    }

    #[test]
    fn a_step_is_exactly_one_thing() {
        let e = err(r#"
name: T
models:
  thing: { fields: { x: { type: text } } }
actions:
  go:
    steps:
      - { delete: ~, notify: "hi" }
"#);
        assert!(e.message.contains("exactly one"), "{e}");
    }

    #[test]
    fn an_unknown_step_names_what_was_expected() {
        let e = err(r#"
name: T
models:
  thing: { fields: { x: { type: text } } }
actions:
  go:
    steps:
      - email: { to: "someone" }
"#);
        assert!(e.message.contains("\"email\""), "{e}");
        assert!(e.message.contains("create_task"), "{e}");
    }

    #[test]
    fn a_field_cannot_be_both_defaulted_and_computed() {
        let e = err(r#"
name: T
models:
  thing:
    fields:
      x: { type: int, default: 1, compute: "1 + 1" }
"#);
        assert_eq!(e.at, "models.thing.fields.x");
        assert!(e.message.contains("could never be seen"), "{e}");
    }

    #[test]
    fn a_repeated_key_is_refused_rather_than_letting_the_last_one_win() {
        // The failure worth preventing: a field declared twice, where "the last
        // one wins" means a column quietly changing type between two readings
        // of the same file. serde_yaml refuses the document outright, which is
        // the outcome we want — pinned here because it is a property of the
        // parser rather than of this module, and a future swap could lose it.
        let e = err(r#"
name: T
models:
  thing:
    fields:
      x: { type: text }
      x: { type: int }
"#);
        assert!(e.at.is_empty(), "reported by the YAML layer, not by ours: {e}");
        assert!(e.message.contains("duplicate"), "{e}");
    }

    #[test]
    fn an_unknown_scope_is_refused_with_its_position() {
        let e = err("name: T\nscopes: [read:board, read:everything]\n");
        assert_eq!(e.at, "scopes[1]");
        assert!(e.message.contains("read:everything"), "{e}");
    }

    #[test]
    fn a_menu_entry_must_point_at_a_real_view() {
        let e = err(r#"
name: T
models:
  thing: { fields: { x: { type: text } } }
views:
  list: { columns: [x] }
menu:
  - { label: Nope, view: missing }
"#);
        assert_eq!(e.at, "menu[0].view");
        assert!(e.message.contains("\"missing\""), "{e}");
    }

    #[test]
    fn a_runtime_that_is_not_one_of_the_three_is_refused() {
        let e = err("name: T\nruntime: python\n");
        assert_eq!(e.at, "runtime");
        assert!(e.message.contains("python"), "{e}");
        assert!(Runtime::Node.is_container());
        assert!(Runtime::Static.is_container());
        assert!(!Runtime::Module.is_container());
    }

    #[test]
    fn a_port_outside_the_real_range_is_refused() {
        assert_eq!(err("name: T\nport: 0\n").at, "port");
        assert_eq!(err("name: T\nport: 70000\n").at, "port");
        assert_eq!(parse("name: T\nport: 3000\n").unwrap().port, Some(3000));
    }

    #[test]
    fn a_model_with_no_fields_is_not_a_model() {
        assert!(err("name: T\nmodels:\n  thing: {}\n").message.contains("at least one field"));
    }

    #[test]
    fn every_field_type_has_a_postgres_type_and_reads_back() {
        for text in ["text", "int", "decimal", "bool", "date", "datetime", "json"] {
            let ty = FieldType::parse(text).unwrap_or_else(|| panic!("{text} should parse"));
            assert_eq!(ty.as_str(), text);
            assert!(!ty.sql().is_empty());
        }
        assert_eq!(FieldType::parse("ref:order").unwrap().sql(), "UUID");
        assert_eq!(FieldType::Ref("order".into()).as_str(), "ref:order");
        assert_eq!(FieldType::parse("blob"), None);
    }

    #[test]
    fn a_manifest_that_is_not_yaml_says_so_rather_than_panicking() {
        let e = err("name: [unclosed\n");
        assert!(e.at.is_empty());
        assert!(e.message.contains("valid YAML"), "{e}");
    }
}
