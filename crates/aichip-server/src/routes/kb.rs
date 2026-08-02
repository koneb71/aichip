//! The knowledge base: articles people write, or ask an agent to write.
//!
//! Bodies are HTML from a rich-text editor, so they are sanitised **on the way
//! in** and stored clean — see `aichip_core::kb::sanitize` for why that
//! direction is the one that matters.
//!
//! Images and files pasted into an article go to object storage rather than
//! into the row. Uploads are served back through this API rather than by
//! handing out a presigned URL: the browser then needs no credentials, the
//! bucket stays private, and a link in an article keeps working without a
//! signature that expires.

use super::{internal, ApiError};
use crate::AppState;
use aichip_core::kb::{diff, render, revisions, tree};
use aichip_core::storage::{object_key, MAX_OBJECT_BYTES};
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/kb/articles", get(list).post(create))
        .route("/kb/articles/{id}", get(one).patch(update).delete(remove))
        .route("/kb/tree", get(page_tree))
        .route("/kb/spaces", get(spaces))
        .route("/kb/articles/{id}/move", post(move_page))
        .route("/kb/articles/{id}/revisions", get(revision_list))
        .route("/kb/articles/{id}/diff", get(revision_diff))
        .route("/kb/articles/{id}/revisions/{seq}/accept", post(accept_revision))
        .route("/kb/articles/{id}/revisions/{seq}/discard", post(discard_revision))
        .route("/kb/articles/{id}/restore", post(restore_revision))
        .route("/kb/articles/{id}/generate", post(regenerate))
        .route("/kb/generate", post(generate))
        .route(
            "/kb/assets",
            post(upload).layer(DefaultBodyLimit::max(MAX_OBJECT_BYTES + 1024 * 1024)),
        )
        .route("/kb/assets/{id}", get(serve_asset))
}

fn article_row(r: &sqlx::postgres::PgRow, with_body: bool) -> Value {
    let mut v = json!({
        "id": r.get::<Uuid, _>("id"),
        "workspaceId": r.get::<Uuid, _>("workspace_id"),
        "title": r.get::<String, _>("title"),
        "summary": r.get::<String, _>("summary"),
        "status": r.get::<String, _>("status"),
        "parentId": r.get::<Option<Uuid>, _>("parent_id"),
        "projectId": r.get::<Option<Uuid>, _>("project_id"),
        "icon": r.get::<String, _>("icon"),
        "position": r.get::<f64, _>("position"),
        "currentSeq": r.get::<i32, _>("current_seq"),
        "bodyVersion": r.get::<i64, _>("body_version"),
        "origin": r.get::<String, _>("origin"),
        "sourceRunId": r.get::<Option<Uuid>, _>("source_run_id"),
        "updatedAt": r.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
    });
    // The list view never needs bodies, and sending fifty of them would make
    // the page slow for no visible benefit.
    if with_body {
        v["contentHtml"] = json!(r.get::<String, _>("content_html"));
    }
    v
}

#[derive(Deserialize)]
struct ListFilter {
    workspace_id: Option<Uuid>,
    /// Free text over titles and summaries.
    q: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(filter): Query<ListFilter>,
) -> Result<Json<Value>, ApiError> {
    let query = filter.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    // Prefix matching and ranking, both of which the plain query lacked: an
    // autocompleter that cannot match "roll" to "Rollback" is not an
    // autocompleter, and an unranked result set made the A/B/D weights on the
    // search vector purely decorative.
    let rows = sqlx::query(
        "SELECT *, CASE WHEN $2::text IS NULL THEN 0
                        ELSE ts_rank(search, websearch_to_tsquery('english', $2)) END AS rank
         FROM kb_articles
         WHERE ($1::uuid IS NULL OR workspace_id = $1)
           AND ($2::text IS NULL
                OR search @@ websearch_to_tsquery('english', $2)
                OR title ILIKE '%' || $2 || '%')
         ORDER BY rank DESC, updated_at DESC
         LIMIT 200",
    )
    .bind(filter.workspace_id)
    .bind(query)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    Ok(Json(json!({
        "articles": rows.iter().map(|r| article_row(r, false)).collect::<Vec<_>>()
    })))
}

/// One page, with everything the read view puts around it.
///
/// In one response rather than five: a page view that fires a request per
/// panel shows its breadcrumb, its children and its backlinks arriving at
/// different moments, and the layout jumps under the reader each time.
async fn one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let row = reload(&state, id).await?;
    let mut page = article_row(&row, true);

    let crumbs = tree::breadcrumb(&state.db, id).await.map_err(internal)?;
    page["breadcrumb"] = json!(crumbs
        .iter()
        .map(|c| json!({ "id": c.id, "title": c.title, "icon": c.icon }))
        .collect::<Vec<_>>());

    let children = sqlx::query(
        "SELECT id, title, icon, summary FROM kb_articles
          WHERE parent_id = $1 ORDER BY position, created_at",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    page["children"] = json!(children
        .iter()
        .map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "title": r.get::<String, _>("title"),
            "icon": r.get::<String, _>("icon"),
            "summary": r.get::<String, _>("summary"),
        }))
        .collect::<Vec<_>>());

    // What links here. The direction people actually read, and the reason
    // linking pages together is worth doing at all.
    let backlinks = sqlx::query(
        "SELECT a.id, a.title, a.icon FROM kb_links l
           JOIN kb_articles a ON a.id = l.from_id
          WHERE l.to_id = $1 ORDER BY a.title",
    )
    .bind(id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;
    page["backlinks"] = json!(backlinks
        .iter()
        .map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "title": r.get::<String, _>("title"),
            "icon": r.get::<String, _>("icon"),
        }))
        .collect::<Vec<_>>());

    page["usedBy"] = used_by(&state, id).await?;

    page["pendingRevision"] = match revisions::pending(&state.db, id).await.map_err(internal)? {
        Some(rev) => revision_json(&rev),
        None => Value::Null,
    };

    // Whether a generation run is still working on this page, so the view can
    // say so instead of showing a blank article.
    page["writing"] = json!(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM runs
                         WHERE kb_article_id = $1
                           AND status IN ('queued','starting','running'))",
    )
    .bind(id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?);

    Ok(Json(page))
}

/// How many referencing cards a page lists before it stops.
///
/// A page that thirty cards depend on has already made its point; the number is
/// there to keep one popular runbook from rendering a screen of links nobody
/// scrolls. The response carries the true total so the view can say what it left
/// out rather than quietly ending the list.
const USED_BY_LIMIT: i64 = 30;

/// Which tasks depend on this page.
///
/// The counterpart to backlinks, and the question a wiki cannot answer about
/// itself: `kb_links` records page→page, but a page attached to a card is a
/// reference living entirely outside the wiki. Without this, the only way to
/// find out whether rewriting a runbook would change what an agent gets handed
/// was to open every card and look.
///
/// Attachments and comment mentions are folded into one row per task because
/// they are the same fact to a reader — *this card uses this page* — but they
/// are kept distinguishable, because they are not the same fact to an agent: an
/// attachment is injected into every run on that card, while a mention reached
/// exactly one reply.
async fn used_by(state: &AppState, id: Uuid) -> Result<Value, ApiError> {
    let rows = sqlx::query(
        "WITH refs AS (
             SELECT task_id, true AS attached FROM task_articles WHERE article_id = $1
             UNION ALL
             SELECT c.task_id, false FROM comment_articles ca
               JOIN task_comments c ON c.id = ca.comment_id
              WHERE ca.article_id = $1
         ),
         rolled AS (
             SELECT task_id,
                    bool_or(attached) AS attached,
                    count(*) FILTER (WHERE NOT attached) AS mentions
               FROM refs GROUP BY task_id
         )
         SELECT t.id, t.title, t.board_column, t.project_id, t.created_at,
                p.name AS project_name, r.attached, r.mentions,
                count(*) OVER () AS total
           FROM rolled r
           JOIN tasks t ON t.id = r.task_id
           JOIN projects p ON p.id = t.project_id
          -- Attached first: those are the cards where this page is part of the
          -- brief, not something somebody once linked in a reply.
          ORDER BY r.attached DESC, t.created_at DESC
          LIMIT $2",
    )
    .bind(id)
    .bind(USED_BY_LIMIT)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let total = rows
        .first()
        .map(|r| r.get::<i64, _>("total"))
        .unwrap_or(0);
    Ok(json!({
        "total": total,
        "tasks": rows.iter().map(|r| json!({
            "id": r.get::<Uuid, _>("id"),
            "title": r.get::<String, _>("title"),
            "projectId": r.get::<Uuid, _>("project_id"),
            "projectName": r.get::<String, _>("project_name"),
            "boardColumn": r.get::<String, _>("board_column"),
            "attached": r.get::<bool, _>("attached"),
            "mentions": r.get::<i64, _>("mentions"),
        })).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
struct NewArticle {
    workspace_id: Uuid,
    title: String,
    #[serde(default)]
    content_html: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    parent_id: Option<Uuid>,
    #[serde(default)]
    project_id: Option<Uuid>,
    #[serde(default)]
    icon: Option<String>,
    /// Asset ids uploaded while composing, bound to the article on save so the
    /// sweeper can tell a live image from an abandoned paste.
    #[serde(default)]
    asset_ids: Vec<Uuid>,
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<NewArticle>,
) -> Result<Json<Value>, ApiError> {
    if body.title.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "a title is required".into()));
    }
    if let Some(parent) = body.parent_id {
        let depth = tree::depth_of(&state.db, parent).await.map_err(internal)? + 1;
        if depth > tree::MAX_DEPTH {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("pages can nest {} deep; this would be one more", tree::MAX_DEPTH + 1),
            ));
        }
    }
    // The row is created empty and the body arrives as revision 1, so there is
    // exactly one path a body can take into the database.
    let row = sqlx::query(
        "INSERT INTO kb_articles (workspace_id, title, status, parent_id, project_id, icon,
                                  position)
         VALUES ($1,$2,$3,$4,$5,$6,
                 COALESCE((SELECT max(position) + 1000 FROM kb_articles
                            WHERE workspace_id = $1
                              AND parent_id IS NOT DISTINCT FROM $4), 1000))
         RETURNING *",
    )
    .bind(body.workspace_id)
    .bind(body.title.trim())
    .bind(body.status.as_deref().unwrap_or("draft"))
    .bind(body.parent_id)
    .bind(body.project_id)
    .bind(body.icon.as_deref().unwrap_or(""))
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;
    let id: Uuid = row.get("id");
    claim_assets(&state, id, &body.asset_ids).await?;

    if !body.content_html.trim().is_empty() {
        // Unguarded, and safe to be: the row was inserted two statements ago
        // and its id has not left this function, so there is no second editor
        // whose work this could be standing on.
        write_body(&state, id, body.title.trim(), &body.content_html, None, None).await?;
    }
    Ok(Json(article_row(&reload(&state, id).await?, true)))
}

#[derive(Deserialize)]
struct ArticlePatch {
    title: Option<String>,
    content_html: Option<String>,
    status: Option<String>,
    icon: Option<String>,
    /// Present-but-null files the page under the workspace-wide space.
    #[serde(default, deserialize_with = "double_option")]
    project_id: Option<Option<Uuid>>,
    /// The revision the editor loaded, recorded on the new revision as what it
    /// is a change against. It is the *diff anchor*, not the concurrency guard
    /// — see `base_version`.
    base_seq: Option<i32>,
    /// `bodyVersion` as the editor loaded it. A body write without it would
    /// overwrite whatever arrived in the meantime, which is the whole failure
    /// this design exists to remove.
    base_version: Option<i64>,
    #[serde(default)]
    asset_ids: Vec<Uuid>,
}

fn double_option<'de, D>(de: D) -> Result<Option<Option<Uuid>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(de).map(Some)
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<ArticlePatch>,
) -> Result<Json<Value>, ApiError> {
    // The body goes first, because it is the write that can be refused. Doing
    // metadata first meant a rejected save still renamed the page — the change
    // the user was told did not happen.
    if let Some(html) = body.content_html.as_deref() {
        // Falls back to the stored title when the incoming one is blank, so
        // the two paths agree: the metadata branch already refuses to blank a
        // title, and the body branch must not be a way around it.
        let title = match body.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => t.to_string(),
            None => sqlx::query_scalar("SELECT title FROM kb_articles WHERE id=$1")
                .bind(id)
                .fetch_optional(&state.db.pool)
                .await
                .map_err(internal)?
                .ok_or((StatusCode::NOT_FOUND, "no such page".to_string()))?,
        };
        write_body(&state, id, &title, html, body.base_seq, body.base_version).await?;
    }

    // Everything that is not the body: metadata a person changes from the page
    // header, which has no bearing on the revision log.
    if body.title.is_some()
        || body.status.is_some()
        || body.icon.is_some()
        || body.project_id.is_some()
    {
        sqlx::query(
            "UPDATE kb_articles SET
                title = COALESCE($2, title),
                status = COALESCE($3, status),
                icon = COALESCE($4, icon),
                project_id = CASE WHEN $6 THEN $5 ELSE project_id END,
                updated_at = now()
             WHERE id = $1",
        )
        .bind(id)
        .bind(body.title.as_deref().map(str::trim).filter(|t| !t.is_empty()))
        .bind(body.status.as_deref())
        .bind(body.icon.as_deref())
        .bind(body.project_id.flatten())
        .bind(body.project_id.is_some())
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    }

    claim_assets(&state, id, &body.asset_ids).await?;
    Ok(Json(article_row(&reload(&state, id).await?, true)))
}

/// The one path a body takes into the database.
///
/// Sanitise, project, and record a revision — never a bare UPDATE. A caller
/// that writes `content_html` directly would skip the search projection, the
/// backlink rebuild and the history all at once.
async fn write_body(
    state: &AppState,
    id: Uuid,
    title: &str,
    html: &str,
    base_seq: Option<i32>,
    base_version: Option<i64>,
) -> Result<(), ApiError> {
    let prepared = render::prepare(html);
    let rev = revisions::NewRevision {
        title,
        html: &prepared.html,
        text: &prepared.text,
        author: revisions::Author::Human,
        kind: "edit",
        base_seq,
        run_id: None,
        note: "",
    };
    revisions::save_edit(&state.db, id, rev, base_version).await.map_err(|e| {
        // A stale editor is a 409 the UI turns into a diff, not a 500.
        match e.downcast_ref::<revisions::Conflict>() {
            Some(c) => (StatusCode::CONFLICT, c.to_string()),
            None => internal(e),
        }
    })?;
    Ok(())
}

async fn reload(state: &AppState, id: Uuid) -> Result<sqlx::postgres::PgRow, ApiError> {
    sqlx::query("SELECT * FROM kb_articles WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such page".to_string()))
}

async fn remove(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    // Children are lifted to where their parent was, and the object keys are
    // collected first — the foreign key is RESTRICT precisely so a delete can
    // never take a subtree, and its assets, down with it silently.
    let keys = tree::delete_reparenting(&state.db, id)
        .await
        .map_err(internal)?;

    if let Some(storage) = &state.storage {
        for key in keys {
            if let Err(e) = storage.delete(&key).await {
                tracing::warn!(%key, error = %e, "could not remove knowledge-base object");
            }
        }
    }
    Ok(Json(json!({ "deleted": true })))
}

/// Bind uploads to the article that now contains them.
async fn claim_assets(state: &AppState, article_id: Uuid, ids: &[Uuid]) -> Result<(), ApiError> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query("UPDATE kb_assets SET article_id = $1 WHERE id = ANY($2) AND article_id IS NULL")
        .bind(article_id)
        .bind(ids)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;
    Ok(())
}

// ── Structure ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TreeFilter {
    workspace_id: Uuid,
    /// The space to show. Absent means the workspace-wide "General" space.
    project_id: Option<Uuid>,
}

/// Every page in one space, flat and correctly ordered.
///
/// Flat rather than nested because the client has to nest it anyway to render
/// collapse state, and a flat list makes "everything under this page" a filter
/// instead of a walk.
async fn page_tree(
    State(state): State<AppState>,
    Query(f): Query<TreeFilter>,
) -> Result<Json<Value>, ApiError> {
    let nodes = tree::of_space(&state.db, f.workspace_id, f.project_id)
        .await
        .map_err(internal)?;
    Ok(Json(json!({
        "pages": nodes.iter().map(|n| json!({
            "id": n.id,
            "parentId": n.parent_id,
            "projectId": n.project_id,
            "title": n.title,
            "icon": n.icon,
            "position": n.position,
            "status": n.status,
            "origin": n.origin,
            "childCount": n.child_count,
            "hasPending": n.has_pending,
            "writing": n.writing,
        })).collect::<Vec<_>>()
    })))
}

/// The spaces a workspace has: one per repository, plus General.
///
/// A space is a repository rather than a third grouping noun invented above
/// projects — the pages worth writing are about a codebase, and scoping them
/// this way is also what stops repo B's card being handed repo A's runbook.
async fn spaces(
    State(state): State<AppState>,
    Query(f): Query<WsFilter>,
) -> Result<Json<Value>, ApiError> {
    let rows = sqlx::query(
        "SELECT p.id, p.name,
                (SELECT count(*) FROM kb_articles a WHERE a.project_id = p.id) AS pages
           FROM projects p
          WHERE p.kind = 'repo' AND p.workspace_id = $1 ORDER BY p.name",
    )
    .bind(f.workspace_id)
    .fetch_all(&state.db.pool)
    .await
    .map_err(internal)?;

    let general: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM kb_articles WHERE workspace_id = $1 AND project_id IS NULL",
    )
    .bind(f.workspace_id)
    .fetch_one(&state.db.pool)
    .await
    .map_err(internal)?;

    let mut spaces = vec![json!({ "id": Value::Null, "name": "General", "pages": general })];
    spaces.extend(rows.iter().map(|r| {
        json!({
            "id": r.get::<Uuid, _>("id"),
            "name": r.get::<String, _>("name"),
            "pages": r.get::<i64, _>("pages"),
        })
    }));
    Ok(Json(json!({ "spaces": spaces })))
}

#[derive(Deserialize)]
struct WsFilter {
    workspace_id: Uuid,
}

#[derive(Deserialize)]
struct Move {
    /// Null means "top level".
    #[serde(default)]
    parent_id: Option<Uuid>,
    /// The sibling to sit after. Absent means first.
    #[serde(default)]
    after_id: Option<Uuid>,
}

async fn move_page(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<Move>,
) -> Result<Json<Value>, ApiError> {
    tree::move_page(&state.db, id, body.parent_id, body.after_id)
        .await
        // Cycles and depth are the two ways a move goes wrong, and both are
        // the user's mistake to correct, not a server fault.
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "moved": true })))
}

// ── History ─────────────────────────────────────────────────────────────────

fn revision_json(r: &revisions::Revision) -> Value {
    json!({
        "seq": r.seq,
        "kind": r.kind,
        "state": r.state,
        "authorKind": r.author_kind,
        "title": r.title,
        "baseSeq": r.base_seq,
        "restoredFrom": r.restored_from,
        "runId": r.run_id,
        "note": r.note,
        "createdAt": r.created_at,
        "chars": r.chars,
    })
}

async fn revision_list(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let revs = revisions::list(&state.db, id).await.map_err(internal)?;
    Ok(Json(json!({
        "revisions": revs.iter().map(revision_json).collect::<Vec<_>>()
    })))
}

#[derive(Deserialize)]
struct DiffRange {
    /// Absent compares against whatever the revision was written against.
    from: Option<i32>,
    to: i32,
}

/// A unified diff between two revisions, in the format the dashboard's
/// existing diff renderer already parses.
async fn revision_diff(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(range): Query<DiffRange>,
) -> Result<Json<Value>, ApiError> {
    let to = revisions::text_of(&state.db, id, range.to)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such revision".to_string()))?;

    let from_seq = match range.from {
        Some(seq) => Some(seq),
        None => {
            let revs = revisions::list(&state.db, id).await.map_err(internal)?;
            revs.iter().find(|r| r.seq == range.to).and_then(|r| r.base_seq)
        }
    };
    let from = match from_seq {
        Some(seq) => revisions::text_of(&state.db, id, seq)
            .await
            .map_err(internal)?
            .unwrap_or_default(),
        None => String::new(),
    };

    let d = diff::delta(&from, &to);
    Ok(Json(json!({
        "from": from_seq,
        "to": range.to,
        "added": d.added,
        "removed": d.removed,
        "diff": diff::unified(
            &from,
            &to,
            &from_seq.map(|s| format!("revision {s}")).unwrap_or_else(|| "empty page".into()),
            &format!("revision {}", range.to),
        ),
    })))
}

async fn accept_revision(
    State(state): State<AppState>,
    Path((id, seq)): Path<(Uuid, i32)>,
) -> Result<Json<Value>, ApiError> {
    let live = revisions::accept(&state.db, id, seq)
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({ "accepted": true, "seq": live })))
}

#[derive(Deserialize)]
struct Discard {
    #[serde(default)]
    note: String,
}

async fn discard_revision(
    State(state): State<AppState>,
    Path((id, seq)): Path<(Uuid, i32)>,
    Json(body): Json<Discard>,
) -> Result<Json<Value>, ApiError> {
    revisions::discard(&state.db, id, seq, body.note.trim())
        .await
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(json!({ "discarded": true })))
}

#[derive(Deserialize)]
struct Restore {
    seq: i32,
}

async fn restore_revision(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<Restore>,
) -> Result<Json<Value>, ApiError> {
    let seq = revisions::restore(&state.db, id, body.seq)
        .await
        .map_err(internal)?;
    Ok(Json(json!({ "restored": true, "seq": seq })))
}

// ── Assets ──────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AssetUpload {
    workspace_id: Uuid,
}

async fn upload(
    State(state): State<AppState>,
    Query(q): Query<AssetUpload>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let Some(storage) = state.storage.clone() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "file storage isn't configured — set AICHIP_S3_ENDPOINT, \
             AICHIP_S3_ACCESS_KEY and AICHIP_S3_SECRET_KEY (see the README) \
             and restart. Articles work without it; attachments don't."
                .into(),
        ));
    };

    let mut saved = vec![];
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("malformed upload: {e}")))?
    {
        let Some(filename) = field.file_name().map(str::to_string) else {
            continue; // not a file part
        };
        let declared = field.content_type().map(str::to_string);
        let bytes = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, format!("{filename}: upload failed: {e}")))?;

        // Sniffed, not trusted. The browser's content-type is a claim by the
        // uploader, and it decides how bytes are served back.
        let content_type = sniff(&bytes, declared.as_deref()).ok_or((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            format!("{filename}: that file type isn't accepted here"),
        ))?;

        let id = Uuid::new_v4();
        let key = object_key(id, &filename);
        storage
            .put(&key, bytes.to_vec(), content_type)
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        sqlx::query(
            "INSERT INTO kb_assets (id, workspace_id, object_key, filename, content_type, size_bytes)
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(id)
        .bind(q.workspace_id)
        .bind(&key)
        .bind(&filename)
        .bind(content_type)
        .bind(bytes.len() as i64)
        .execute(&state.db.pool)
        .await
        .map_err(internal)?;

        saved.push(json!({
            "id": id,
            "filename": filename,
            "contentType": content_type,
            "sizeBytes": bytes.len(),
            // What the editor puts in `src` / `href`.
            "url": format!("/api/kb/assets/{id}"),
        }));
    }

    if saved.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "no file in the upload".into()));
    }
    Ok(Json(json!({ "assets": saved })))
}

/// What an uploaded file actually is, by its bytes.
///
/// An allowlist rather than a denylist, and sniffed rather than believed: the
/// stored content-type is what this API later serves with, so trusting the
/// uploader's claim would let a `.png` come back as `text/html` and run in the
/// reader's origin.
fn sniff(bytes: &[u8], declared: Option<&str>) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    if bytes.starts_with(b"RIFF") && bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Some("application/zip");
    }
    // SVG is XML, so it has no magic bytes worth trusting — and it can carry
    // script. Served as a download rather than inline, it is safe; served
    // inline it is an XSS vector, so it is simply not accepted.
    if declared == Some("image/svg+xml") {
        return None;
    }
    // Plain text, only if it really reads as text. "Valid UTF-8 with no NUL"
    // is not enough on its own — an ELF header passes both and would be filed
    // as a text document. Serving it as text/plain is harmless, but calling a
    // binary "text" is a lie the UI then repeats.
    if looks_like_text(bytes) {
        return Some("text/plain; charset=utf-8");
    }
    None
}

/// Text, by the standard heuristic: decodable, no NUL, and not mostly control
/// characters.
fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return false;
    }
    let controls = bytes
        .iter()
        .filter(|b| b.is_ascii_control() && !matches!(b, b'\t' | b'\n' | b'\r'))
        .count();
    // One in a hundred is already generous for prose; a binary blows past it
    // in its first few bytes.
    controls * 100 <= bytes.len()
}

async fn serve_asset(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, ApiError> {
    let row = sqlx::query("SELECT object_key, content_type, filename FROM kb_assets WHERE id=$1")
        .bind(id)
        .fetch_optional(&state.db.pool)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "no such file".to_string()))?;

    let Some(storage) = &state.storage else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "file storage isn't configured on this server".into(),
        ));
    };
    let key: String = row.get("object_key");
    let bytes = storage
        .get(&key)
        .await
        .map_err(internal)?
        .ok_or((StatusCode::NOT_FOUND, "that file is no longer stored".to_string()))?;

    let content_type: String = row.get("content_type");
    let filename: String = row.get("filename");
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            // Belt and braces against the sniffer being wrong: never let a
            // browser second-guess the type into something executable.
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            // Inline so images render in an article; the filename is still
            // offered for a save-as. `filename*` is skipped deliberately —
            // the name is user input and belongs quoted, not interpolated.
            (
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{}\"", filename.replace(['"', '\\', '\n'], "_")),
            ),
            (header::CACHE_CONTROL, "private, max-age=31536000".to_string()),
        ],
        bytes,
    ))
}

// ── Generation ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Generate {
    workspace_id: Uuid,
    /// Which repository to document.
    project_id: Uuid,
    /// What the article should cover.
    brief: String,
    engine: Option<String>,
    /// Write it inside an existing page.
    #[serde(default)]
    parent_id: Option<Uuid>,
}

async fn generate(
    State(state): State<AppState>,
    Json(body): Json<Generate>,
) -> Result<Json<Value>, ApiError> {
    if body.brief.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "say what the article should cover".into(),
        ));
    }
    let id = state
        .orchestrator
        .enqueue_kb_article(
            body.workspace_id,
            body.project_id,
            body.brief.trim(),
            body.engine.as_deref(),
            None,
            body.parent_id,
        )
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    // The article id as well as the run id: the page row exists immediately,
    // so the caller can navigate to it and watch it fill in.
    let article_id: Option<Uuid> =
        sqlx::query_scalar("SELECT kb_article_id FROM runs WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?
            .flatten();
    Ok(Json(json!({ "runId": id, "articleId": article_id })))
}

#[derive(Deserialize)]
struct Rewrite {
    brief: String,
    project_id: Uuid,
    engine: Option<String>,
}

/// Ask an agent to revise an article that already exists.
async fn regenerate(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<Rewrite>,
) -> Result<Json<Value>, ApiError> {
    let workspace_id: Uuid =
        sqlx::query_scalar("SELECT workspace_id FROM kb_articles WHERE id=$1")
            .bind(id)
            .fetch_optional(&state.db.pool)
            .await
            .map_err(internal)?
            .ok_or((StatusCode::NOT_FOUND, "no such article".to_string()))?;

    let run_id = state
        .orchestrator
        .enqueue_kb_article(
            workspace_id,
            body.project_id,
            body.brief.trim(),
            body.engine.as_deref(),
            Some(id),
            None,
        )
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(json!({ "runId": run_id })))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser's claim is not evidence; the bytes are. An ELF header is
    /// valid UTF-8 with no NUL byte, so the text fallback has to be stricter
    /// than "decodes" or a binary gets filed as a document.
    #[test]
    fn a_renamed_executable_is_not_accepted_as_an_image_or_as_text() {
        assert_eq!(sniff(b"\x7fELF\x02\x01\x01\x00\x00\x00", Some("image/png")), None);
    }

    #[test]
    fn genuine_text_still_gets_through() {
        assert_eq!(
            sniff(b"# Notes\n\nline one\tindented\r\n", None),
            Some("text/plain; charset=utf-8")
        );
    }

    #[test]
    fn real_images_are_recognised() {
        assert_eq!(sniff(&[0x89, b'P', b'N', b'G', 0, 0], None), Some("image/png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0], None), Some("image/jpeg"));
        assert_eq!(sniff(b"%PDF-1.7", None), Some("application/pdf"));
    }

    /// SVG is XML that can carry script, and this API serves assets inline.
    #[test]
    fn svg_is_refused_rather_than_served_inline() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
        assert_eq!(sniff(svg, Some("image/svg+xml")), None);
    }

    /// Without this, an uploaded `.html` would come back as `text/html` in the
    /// app's own origin — which is the whole XSS story again, one layer down.
    #[test]
    fn markup_uploaded_as_text_is_served_as_plain_text() {
        let html = b"<html><script>alert(1)</script></html>";
        assert_eq!(sniff(html, Some("text/html")), Some("text/plain; charset=utf-8"));
    }
}
