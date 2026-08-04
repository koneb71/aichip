//! Serving `/__aichip/*` to a container app.
//!
//! Answered here, in aichip's process, **before anything is forwarded**. The
//! container never sees these requests and needs no network access to the host
//! to make them: the capability belongs to the app's page in the browser, and
//! the page proves which app it is by the hostname it was served from — which
//! it cannot forge, because a forged request would never reach this module.
//!
//! ## Three gates, in order
//!
//! 1. **The header.** Every path but `client.js` and `health` must carry
//!    `X-Aichip-App`. A custom header forces a preflight, and aichip answers no
//!    CORS at all, so a cross-origin caller never gets to send the real
//!    request. This is not belt and braces over the origin check below — it is
//!    the *primary* defence, because a cross-origin `POST` with
//!    `Content-Type: text/plain` is a simple request and would otherwise land.
//! 2. **The origin.** Absent, or exactly this app's own. Closes the resource
//!    loads that carry no `Origin` at all.
//! 3. **The scope.** Deny by default, from an exhaustive enum. See
//!    `apps::bridge`.
//!
//! Nothing here calls `next.run(req)`, so `reject_non_local_callers` is neither
//! consulted nor weakened: this is a separate, far narrower surface that exists
//! only on app hostnames.

use crate::AppState;
use aichip_core::apps::{self, bridge::Route};
use axum::body::Body;
use axum::http::{header, HeaderValue, Request, Response, StatusCode};
use serde_json::{json, Value};
use sqlx::Row;

/// The header every real bridge call carries.
pub const APP_HEADER: &str = "x-aichip-app";

/// How much of a request body the bridge will read.
///
/// Rows go through here, not files. Small on purpose: an app that wants to
/// store four megabytes in a text column is doing something this was not for.
const MAX_BODY: usize = 1024 * 1024;

fn reply(status: StatusCode, body: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        // No `Access-Control-Allow-*`, ever. Their absence is what makes the
        // header gate work: the preflight has nothing to succeed with.
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body.to_string()))
        .expect("a JSON response is always well formed")
}

fn oops(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    reply(status, json!({ "error": message.into() }))
}

/// Handle a request for the reserved prefix.
///
/// `slug` is the app hostname's label; `segments` is the path after
/// `/__aichip`, already refused if it contained a dot segment.
pub async fn handle(
    state: &AppState,
    slug: &str,
    segments: Vec<String>,
    req: Request<Body>,
) -> Response<Body> {
    let refs: Vec<&str> = segments.iter().map(String::as_str).collect();
    let method = req.method().as_str().to_string();
    let route = apps::bridge::route(&method, &refs);

    // `OPTIONS` is answered before anything else and always refused. A
    // permissive preflight is the one thing that would undo the header gate,
    // so it is denied explicitly rather than left to a default.
    if method == "OPTIONS" {
        return oops(StatusCode::FORBIDDEN, "this API is same-origin only");
    }

    if !route.header_exempt() {
        if req.headers().get(APP_HEADER).is_none() {
            return oops(
                StatusCode::FORBIDDEN,
                "this API is only reachable from the app itself — load \
                 /__aichip/client.js and use window.aichip",
            );
        }
        if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            // Exactly this app's own origin. Port-agnostic for the same reason
            // the dashboard's check is: a dev server serves the page from a
            // different one.
            if apps::host::classify(origin).map(|(_, l)| l) != Some(slug) {
                return oops(StatusCode::FORBIDDEN, "this API is same-origin only");
            }
        }
    }

    // Served before an app is looked up, so a browser that cannot resolve
    // `*.localhost` gets a different answer from one whose app is missing.
    match route {
        Route::ClientJs => {
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/javascript; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Body::from(apps::client_js::CLIENT_JS))
                .expect("a static script is always well formed")
        }
        Route::AppCss => {
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
                .header(header::CACHE_CONTROL, "no-store")
                .body(Body::from(apps::skeleton::THEME))
                .expect("a static stylesheet is always well formed")
        }
        Route::Health => return reply(StatusCode::OK, json!({ "ok": true, "slug": slug })),
        Route::Unknown => return oops(StatusCode::NOT_FOUND, "no such thing"),
        Route::WrongMethod => {
            return oops(StatusCode::METHOD_NOT_ALLOWED, "that path does not take this method")
        }
        _ => {}
    }

    let app = match apps::by_slug(&state.db, slug).await {
        Ok(Some(app)) => app,
        Ok(None) => return oops(StatusCode::NOT_FOUND, "no such app"),
        Err(e) => return oops(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !app.active {
        return oops(StatusCode::SERVICE_UNAVAILABLE, "this app is switched off");
    }

    if let Some(needed) = route.scope() {
        let held = apps::grants::of(&state.db, app.id).await.unwrap_or_default();
        if !held.contains(&needed) {
            // Its own shape, not a bare error: a missing permission is
            // something the person can grant, and the client turns this into a
            // message saying so rather than a stack trace.
            return reply(
                StatusCode::FORBIDDEN,
                json!({
                    "error": format!("this app has not been granted \"{needed}\""),
                    "needsScope": needed.as_str(),
                }),
            );
        }
        apps::grants::touch(&state.db, app.id, needed).await.ok();
    }

    let (parts, body) = req.into_parts();
    let query = parts.uri.query().unwrap_or("").to_string();
    let body = match axum::body::to_bytes(body, MAX_BODY).await {
        Ok(b) => b,
        Err(_) => return oops(StatusCode::PAYLOAD_TOO_LARGE, "that request body is too large"),
    };

    match serve(state, &app, route, &query, &body).await {
        Ok(response) => response,
        Err((status, message)) => oops(status, message),
    }
}

type Failed = (StatusCode, String);

async fn serve(
    state: &AppState,
    app: &apps::App,
    route: Route,
    query: &str,
    body: &[u8],
) -> Result<Response<Body>, Failed> {
    let manifest = app
        .parsed()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let model_of = |name: &str| {
        apps::data::model_of(&manifest, name)
            .cloned()
            .map_err(|e| (StatusCode::NOT_FOUND, e.0))
    };
    let json_body = || -> Result<serde_json::Map<String, Value>, Failed> {
        serde_json::from_slice(body)
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("that is not a JSON object: {e}")))
    };
    let uuid = |s: &str| -> Result<uuid::Uuid, Failed> {
        s.parse()
            .map_err(|_| (StatusCode::BAD_REQUEST, "that is not a row id".to_string()))
    };
    let data_err = |e: apps::data::DataError| (StatusCode::BAD_REQUEST, e.0);

    let out = match route {
        Route::Me => {
            let held = apps::grants::of(&state.db, app.id).await.unwrap_or_default();
            // Nothing about the workspace: an app is told who *it* is, not
            // where it lives.
            json!({
                "id": app.id,
                "slug": app.slug,
                "name": app.name,
                "scopes": held.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            })
        }
        Route::Schema => json!({
            "models": manifest.models.iter().map(|m| json!({
                "name": m.name,
                "fields": m.fields.iter().map(|f| json!({
                    "name": f.name,
                    "type": f.ty.as_str(),
                    "required": f.required,
                    "computed": f.compute.is_some(),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),

        Route::DataList(name) => {
            let model = model_of(&name)?;
            let raw = raw_query(query);
            let rows = apps::data::list(&state.db, &app.schema, &model, &raw)
                .await
                .map_err(data_err)?;
            let total = apps::data::count(&state.db, &app.schema, &model, &raw)
                .await
                .map_err(data_err)?;
            json!({ "rows": rows, "total": total })
        }
        Route::DataGet(name, id) => {
            let model = model_of(&name)?;
            apps::data::get(&state.db, &app.schema, &model, uuid(&id)?)
                .await
                .map_err(data_err)?
                .ok_or((StatusCode::NOT_FOUND, "no such row".to_string()))?
        }
        Route::DataCreate(name) => {
            let model = model_of(&name)?;
            apps::data::create(&state.db, &app.schema, &model, &json_body()?)
                .await
                .map_err(data_err)?
        }
        Route::DataUpdate(name, id) => {
            let model = model_of(&name)?;
            apps::data::update(&state.db, &app.schema, &model, uuid(&id)?, &json_body()?)
                .await
                .map_err(data_err)?
                .ok_or((StatusCode::NOT_FOUND, "no such row".to_string()))?
        }
        Route::DataDelete(name, id) => {
            let model = model_of(&name)?;
            let gone = apps::data::delete(&state.db, &app.schema, &model, uuid(&id)?)
                .await
                .map_err(data_err)?;
            if !gone {
                return Err((StatusCode::NOT_FOUND, "no such row".to_string()));
            }
            json!({ "ok": true })
        }

        // Everything below is aichip's data, behind a grant already checked.
        // Each is an explicit projection, never a forwarded response — see the
        // note at the top of `apps::bridge` about `t.prompt`.
        Route::Projects => rows_json(
            state,
            "SELECT id, name, default_branch, vcs FROM projects
              WHERE workspace_id = $1 AND kind = 'repo' ORDER BY name",
            app.workspace_id,
            &["id", "name", "defaultBranch", "vcs"],
        )
        .await?,
        Route::Tasks => rows_json(
            state,
            // No `prompt`. It is text a person typed and may contain anything.
            "SELECT t.id, t.title, t.board_column, t.created_at, p.name AS project
               FROM tasks t JOIN projects p ON p.id = t.project_id
              WHERE p.workspace_id = $1 AND p.kind = 'repo'
              ORDER BY t.created_at DESC LIMIT 500",
            app.workspace_id,
            &["id", "title", "boardColumn", "createdAt", "project"],
        )
        .await?,
        Route::Runs => rows_json(
            state,
            // No transcripts, no prompts, no session ids.
            "SELECT r.id, r.status, r.engine, r.model, r.cost_usd, r.started_at, r.finished_at
               FROM runs r JOIN projects p ON p.id = r.project_id
              WHERE p.workspace_id = $1
              ORDER BY r.created_at DESC LIMIT 500",
            app.workspace_id,
            &["id", "status", "engine", "model", "costUsd", "startedAt", "finishedAt"],
        )
        .await?,
        Route::Spend => rows_json(
            state,
            "SELECT p.name AS project, sum(r.cost_usd) AS cost, count(*) AS runs
               FROM runs r JOIN projects p ON p.id = r.project_id
              WHERE p.workspace_id = $1 AND r.cost_usd IS NOT NULL
              GROUP BY p.name ORDER BY 2 DESC NULLS LAST",
            app.workspace_id,
            &["project", "cost", "runs"],
        )
        .await?,
        Route::Agents => rows_json(
            state,
            // No `system_prompt`: an agent's instructions are not an app's
            // business, and they are the most sensitive column on the table.
            "SELECT id, name, icon, color, engine FROM agents
              WHERE workspace_id = $1 ORDER BY name",
            app.workspace_id,
            &["id", "name", "icon", "color", "engine"],
        )
        .await?,
        Route::KbPages => rows_json(
            state,
            // Text, never `body`: the HTML is markup an agent may have written,
            // and the KB itself diffs on text for the same reason.
            "SELECT a.id, a.title, a.summary, left(a.content_text, 4000) AS text
               FROM kb_articles a
               LEFT JOIN projects p ON p.id = a.project_id
              WHERE COALESCE(p.workspace_id, a.workspace_id) = $1
              ORDER BY a.title LIMIT 200",
            app.workspace_id,
            &["id", "title", "summary", "text"],
        )
        .await?,

        Route::CreateTask => {
            let body = json_body()?;
            let title = body
                .get("title")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or((StatusCode::BAD_REQUEST, "a card needs a title".to_string()))?;
            let prompt = body.get("prompt").and_then(Value::as_str).unwrap_or(title);
            let project = body
                .get("project")
                .and_then(Value::as_str)
                .ok_or((StatusCode::BAD_REQUEST, "say which project".to_string()))?;

            let project_id: Option<uuid::Uuid> = sqlx::query_scalar(
                "SELECT id FROM projects
                  WHERE workspace_id = $1 AND kind = 'repo' AND lower(name) = lower($2) LIMIT 1",
            )
            .bind(app.workspace_id)
            .bind(project)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            let project_id = project_id.ok_or((
                StatusCode::NOT_FOUND,
                format!("there is no project called \"{project}\""),
            ))?;

            // Backlog, always. An app may put a card on the board; making an
            // agent run is a different grant and is not reachable from here at
            // all — not gated, absent.
            let id: uuid::Uuid = sqlx::query_scalar(
                "INSERT INTO tasks (project_id, title, prompt, board_column)
                 VALUES ($1, $2, $3, 'backlog') RETURNING id",
            )
            .bind(project_id)
            .bind(title)
            .bind(prompt)
            .fetch_one(&state.db.pool)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            json!({ "id": id, "boardColumn": "backlog" })
        }

        // Handled before an app was looked up.
        Route::ClientJs | Route::AppCss | Route::Health | Route::Unknown | Route::WrongMethod => {
            json!({ "ok": true })
        }
    };

    Ok(reply(StatusCode::OK, out))
}

/// Run a projection and label its columns.
///
/// Everything is read as text and the shape is built here, so a column type
/// this crate cannot decode — `numeric`, chiefly — does not need a feature
/// flag, and a decimal keeps its digits on the way out.
async fn rows_json(
    state: &AppState,
    sql: &str,
    workspace_id: uuid::Uuid,
    names: &[&str],
) -> Result<Value, Failed> {
    let casted = format!("SELECT * FROM ({sql}) q");
    let rows = sqlx::query(&casted)
        .bind(workspace_id)
        .fetch_all(&state.db.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let out: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut object = serde_json::Map::new();
            for (i, name) in names.iter().enumerate() {
                object.insert((*name).to_string(), column(row, i));
            }
            Value::Object(object)
        })
        .collect();
    Ok(json!({ "rows": out }))
}

/// One column, as JSON, without knowing its type.
///
/// Tried in order and falling through to null. Text last-but-one because
/// Postgres will hand back most things as text if asked, and trying it first
/// would flatten every integer into a string.
fn column(row: &sqlx::postgres::PgRow, i: usize) -> Value {
    if let Ok(v) = row.try_get::<Option<uuid::Uuid>, _>(i) {
        return v.map_or(Value::Null, |v| json!(v));
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(i) {
        return v.map_or(Value::Null, |v| json!(v));
    }
    if let Ok(v) = row.try_get::<Option<f64>, _>(i) {
        return v.map_or(Value::Null, |v| json!(v));
    }
    if let Ok(v) = row.try_get::<Option<bool>, _>(i) {
        return v.map_or(Value::Null, |v| json!(v));
    }
    if let Ok(v) = row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(i) {
        return v.map_or(Value::Null, |v| json!(v));
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(i) {
        return v.map_or(Value::Null, Value::String);
    }
    Value::Null
}

/// Read the bridge's query string into the shape `apps::query` wants.
///
/// A list of pairs, not a struct: `where` repeats, and a struct keeps only the
/// last one — which would silently return the wrong rows rather than erroring.
fn raw_query(query: &str) -> apps::query::Raw {
    let mut raw = apps::query::Raw::default();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value);
        match key {
            "where" => raw.filters.push(value),
            "order" => raw.order = Some(value),
            "limit" => raw.limit = value.parse().ok(),
            "offset" => raw.offset = value.parse().ok(),
            _ => {}
        }
    }
    raw
}

/// Undo percent-encoding, and `+` for a space.
///
/// Written out rather than pulled in: this decodes one query string in one
/// place, and everything it produces is a *value* that `apps::query` binds —
/// so a byte it gets wrong is a filter that does not match, never a statement
/// that does something else.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The policy every app response carries.
///
/// `connect-src 'self'` is the one that matters: the app may reach its own
/// origin, where `/__aichip/*` lives, and nowhere else. It does not make a
/// granted read safe — nothing does, once code can read something it can also
/// find a way to shout it — but it turns "post my board anywhere" into a
/// covert-channel problem rather than one line of JavaScript.
///
/// Appended rather than replacing whatever the app sent: multiple CSP headers
/// all apply and the intersection wins, so an app with a stricter policy of its
/// own keeps it.
pub fn csp() -> HeaderValue {
    HeaderValue::from_static(
        "default-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob:; \
         connect-src 'self'; form-action 'self'; base-uri 'self'; \
         frame-ancestors http://localhost:* http://127.0.0.1:*",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_pins_an_app_to_its_own_origin() {
        let csp = csp().to_str().unwrap().to_string();
        assert!(csp.contains("connect-src 'self'"), "{csp}");
        assert!(csp.contains("form-action 'self'"), "a form is a way out too: {csp}");
        // …and the dashboard has to be able to embed it, or the iframe is blank.
        assert!(csp.contains("frame-ancestors http://localhost:*"), "{csp}");
    }

    #[test]
    fn a_query_string_survives_decoding() {
        let raw = raw_query("where=note%3Alike%3A50%25&order=-created_at&limit=10");
        assert_eq!(raw.filters, vec!["note:like:50%"]);
        assert_eq!(raw.order.as_deref(), Some("-created_at"));
        assert_eq!(raw.limit, Some(10));
    }

    #[test]
    fn a_repeated_filter_is_kept_rather_than_replaced() {
        // Keeping only the last would silently return the wrong rows.
        let raw = raw_query("where=a%3Aeq%3A1&where=b%3Aeq%3A2");
        assert_eq!(raw.filters.len(), 2);
    }

    #[test]
    fn a_plus_is_a_space_and_a_stray_percent_is_itself() {
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
    }
}
