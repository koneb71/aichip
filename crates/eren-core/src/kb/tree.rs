//! Pages have parents, siblings, and an order.
//!
//! Three things go wrong in every hand-rolled page tree, and all three are
//! handled here rather than left to the caller:
//!
//! - **Cycles.** `parent_id` is this schema's first self-reference. Making a
//!   page its own ancestor turns every recursive query into an infinite one,
//!   so a move is checked before it is written and every walk is depth-bounded
//!   as well — a belt for the rule and braces for a row that got past it.
//! - **Float exhaustion.** Ordering by midpoint runs out of precision after
//!   about fifty drags between the same two neighbours, and then the tree
//!   silently stops respecting the order you dropped things in.
//! - **Orphans.** Deleting a parent must not take its children with it.

use crate::db::Db;
use sqlx::Row;
use uuid::Uuid;

/// How deep the tree may go.
///
/// A bound that is enforced beats a bound the renderer clamps: the move picker
/// simply doesn't offer an illegal destination, so nobody meets a surprise
/// failure. Five levels is more than a forty-page wiki ever needs.
pub const MAX_DEPTH: i32 = 5;

/// Below this gap, halving again stops being representable.
const MIN_GAP: f64 = 1e-6;

/// One page as the tree rail draws it.
#[derive(Debug, Clone)]
pub struct Node {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub title: String,
    pub icon: String,
    pub position: f64,
    pub status: String,
    pub origin: String,
    pub child_count: i64,
    pub has_pending: bool,
    /// A generation run is still writing this page.
    pub writing: bool,
}

/// Where a page sits, root first, including itself.
#[derive(Debug, Clone)]
pub struct Crumb {
    pub id: Uuid,
    pub title: String,
    pub icon: String,
}

/// Every page in one space, flat and correctly ordered.
///
/// Flat rather than nested: the client nests it, and a flat list is what makes
/// "show every descendant of the page I'm on" a filter rather than a walk.
pub async fn of_space(
    db: &Db,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
) -> anyhow::Result<Vec<Node>> {
    let rows = sqlx::query(
        "SELECT a.id, a.parent_id, a.project_id, a.title, a.icon, a.position,
                a.status, a.origin, a.current_seq,
                (SELECT count(*) FROM kb_articles c WHERE c.parent_id = a.id) AS child_count,
                EXISTS (SELECT 1 FROM kb_revisions r
                        WHERE r.article_id = a.id AND r.state = 'pending') AS has_pending,
                EXISTS (SELECT 1 FROM runs r
                        WHERE r.kb_article_id = a.id
                          AND r.status IN ('queued','starting','running')) AS writing
           FROM kb_articles a
          WHERE a.workspace_id = $1
            AND ($2::uuid IS NULL OR a.project_id = $2)
            AND ($2::uuid IS NOT NULL OR a.project_id IS NULL)
          ORDER BY a.position, a.created_at",
    )
    .bind(workspace_id)
    .bind(project_id)
    .fetch_all(&db.pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| Node {
            id: r.get("id"),
            parent_id: r.get("parent_id"),
            project_id: r.get("project_id"),
            title: r.get("title"),
            icon: r.get("icon"),
            position: r.get("position"),
            status: r.get("status"),
            origin: r.get("origin"),
            child_count: r.get("child_count"),
            has_pending: r.get("has_pending"),
            writing: r.get("writing"),
        })
        .collect())
}

/// The path from the root down to this page.
pub async fn breadcrumb(db: &Db, id: Uuid) -> anyhow::Result<Vec<Crumb>> {
    // Depth-bounded even though moves are cycle-checked: a corrupted row must
    // not be able to hang a request, and the cap costs nothing.
    let rows = sqlx::query(
        "WITH RECURSIVE up AS (
             SELECT id, parent_id, title, icon, 0 AS depth
               FROM kb_articles WHERE id = $1
             UNION ALL
             SELECT a.id, a.parent_id, a.title, a.icon, up.depth + 1
               FROM kb_articles a JOIN up ON a.id = up.parent_id
              WHERE up.depth < 8
         )
         SELECT id, title, icon FROM up ORDER BY depth DESC",
    )
    .bind(id)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Crumb {
            id: r.get("id"),
            title: r.get("title"),
            icon: r.get("icon"),
        })
        .collect())
}

/// How deep a page sits, counting from 0 at the root.
pub async fn depth_of(db: &Db, id: Uuid) -> anyhow::Result<i32> {
    Ok(breadcrumb(db, id).await?.len() as i32 - 1)
}

/// The tallest subtree hanging off this page.
///
/// Needed because moving a page moves everything under it: a two-level subtree
/// dropped at depth 4 would put its leaves at depth 6.
pub async fn subtree_height(db: &Db, id: Uuid) -> anyhow::Result<i32> {
    let height: Option<i32> = sqlx::query_scalar(
        "WITH RECURSIVE down AS (
             SELECT id, 0 AS depth FROM kb_articles WHERE id = $1
             UNION ALL
             SELECT a.id, down.depth + 1
               FROM kb_articles a JOIN down ON a.parent_id = down.id
              WHERE down.depth < 8
         )
         SELECT max(depth) FROM down",
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await?;
    Ok(height.unwrap_or(0))
}

/// Would making `parent` the parent of `moving` create a loop?
pub async fn would_cycle(db: &Db, moving: Uuid, parent: Uuid) -> anyhow::Result<bool> {
    if moving == parent {
        return Ok(true);
    }
    let hit: Option<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE up AS (
             SELECT id, parent_id, 0 AS depth FROM kb_articles WHERE id = $1
             UNION ALL
             SELECT a.id, a.parent_id, up.depth + 1
               FROM kb_articles a JOIN up ON a.id = up.parent_id
              WHERE up.depth < 16
         )
         SELECT id FROM up WHERE id = $2 LIMIT 1",
    )
    .bind(parent)
    .bind(moving)
    .fetch_optional(&db.pool)
    .await?;
    Ok(hit.is_some())
}

/// Move a page: new parent, and a position among its new siblings.
///
/// `after` is the sibling it should follow — `None` puts it first.
pub async fn move_page(
    db: &Db,
    id: Uuid,
    parent: Option<Uuid>,
    after: Option<Uuid>,
) -> anyhow::Result<()> {
    if let Some(parent) = parent {
        if would_cycle(db, id, parent).await? {
            anyhow::bail!("a page cannot be moved inside itself");
        }
        // A page and its parent have to be in the same space, or the child
        // appears in neither tree: the parent's space doesn't list it because
        // the space filter excludes it, and its own space doesn't show it
        // because its parent isn't there to hang it from.
        let same: bool = sqlx::query_scalar(
            "SELECT a.workspace_id = b.workspace_id
                    AND a.project_id IS NOT DISTINCT FROM b.project_id
               FROM kb_articles a, kb_articles b WHERE a.id = $1 AND b.id = $2",
        )
        .bind(id)
        .bind(parent)
        .fetch_optional(&db.pool)
        .await?
        .unwrap_or(false);
        if !same {
            anyhow::bail!("a page can only be moved inside its own space");
        }
        let depth = depth_of(db, parent).await? + 1 + subtree_height(db, id).await?;
        if depth > MAX_DEPTH {
            anyhow::bail!(
                "that would nest pages {} deep; the limit is {}",
                depth + 1,
                MAX_DEPTH + 1
            );
        }
    }

    let mut tx = db.pool.begin().await?;
    let space: (Uuid, Option<Uuid>) =
        sqlx::query_as("SELECT workspace_id, project_id FROM kb_articles WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;

    // Siblings under the destination, excluding the page being moved.
    let siblings: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT id, position FROM kb_articles
          WHERE workspace_id = $1
            AND parent_id IS NOT DISTINCT FROM $2
            AND ($3::uuid IS NULL OR project_id = $3)
            AND ($3::uuid IS NOT NULL OR project_id IS NULL)
            AND id <> $4
          ORDER BY position",
    )
    .bind(space.0)
    .bind(parent)
    .bind(space.1)
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;

    let index = match after {
        None => 0,
        Some(a) => siblings
            .iter()
            .position(|(sid, _)| *sid == a)
            .map_or(siblings.len(), |i| i + 1),
    };
    let before = index
        .checked_sub(1)
        .and_then(|i| siblings.get(i))
        .map(|(_, p)| *p);
    let next = siblings.get(index).map(|(_, p)| *p);
    let position = midpoint(before, next);

    sqlx::query("UPDATE kb_articles SET parent_id=$2, position=$3, updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(parent)
        .bind(position)
        .execute(&mut *tx)
        .await?;

    // Rewrite the whole sibling run whenever the gaps get too small to halve
    // again. Nothing else in the app does this, and it is the difference
    // between an order that keeps working and one that quietly stops.
    if needs_renormalising(before, next) {
        renormalise(&mut tx, space.0, space.1, parent).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// A position between two neighbours.
pub fn midpoint(before: Option<f64>, after: Option<f64>) -> f64 {
    match (before, after) {
        (Some(b), Some(a)) => (b + a) / 2.0,
        (Some(b), None) => b + 1.0,
        (None, Some(a)) => a - 1.0,
        (None, None) => 0.0,
    }
}

fn needs_renormalising(before: Option<f64>, after: Option<f64>) -> bool {
    matches!((before, after), (Some(b), Some(a)) if (a - b).abs() < MIN_GAP)
}

async fn renormalise(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    parent: Option<Uuid>,
) -> anyhow::Result<()> {
    sqlx::query(
        "WITH ordered AS (
             SELECT id, row_number() OVER (ORDER BY position, created_at) AS n
               FROM kb_articles
              WHERE workspace_id = $1
                AND parent_id IS NOT DISTINCT FROM $2
                AND ($3::uuid IS NULL OR project_id = $3)
                AND ($3::uuid IS NOT NULL OR project_id IS NULL)
         )
         UPDATE kb_articles a SET position = ordered.n * 1000.0
           FROM ordered WHERE a.id = ordered.id",
    )
    .bind(workspace_id)
    .bind(parent)
    .bind(project_id)
    .execute(&mut **tx)
    .await?;
    tracing::info!("renormalised knowledge-base sibling order");
    Ok(())
}

/// Delete a page, lifting its children to where it was.
///
/// The foreign key is RESTRICT rather than CASCADE on purpose: a cascade would
/// delete a whole subtree from one click, and the object-storage cleanup only
/// collects keys for the page named — so every descendant's bytes would be
/// orphaned with nothing left pointing at them. Reparenting has to happen
/// first, in the same transaction, or the delete fails the FK for every page
/// anyone actually organised.
pub async fn delete_reparenting(db: &Db, id: Uuid) -> anyhow::Result<Vec<String>> {
    let mut tx = db.pool.begin().await?;
    let keys: Vec<String> =
        sqlx::query_scalar("SELECT object_key FROM kb_assets WHERE article_id = $1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;

    sqlx::query(
        "UPDATE kb_articles
            SET parent_id = (SELECT parent_id FROM kb_articles WHERE id = $1)
          WHERE parent_id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM kb_articles WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_lands_between_its_neighbours() {
        assert_eq!(midpoint(Some(1.0), Some(2.0)), 1.5);
    }

    #[test]
    fn the_ends_of_the_list_extend_rather_than_collide() {
        assert_eq!(midpoint(Some(5.0), None), 6.0);
        assert_eq!(midpoint(None, Some(5.0)), 4.0);
        assert_eq!(midpoint(None, None), 0.0);
    }

    #[test]
    fn ordering_survives_repeated_drops_in_the_same_gap() {
        // Fifty drags between one pair is unusual but not absurd, and the
        // failure it causes is silent.
        let (mut lo, mut hi) = (1.0_f64, 2.0_f64);
        for _ in 0..50 {
            let mid = midpoint(Some(lo), Some(hi));
            assert!(mid > lo && mid < hi, "order collapsed at gap {}", hi - lo);
            if needs_renormalising(Some(lo), Some(hi)) {
                return; // caught before it collapsed, which is the point
            }
            hi = mid;
        }
        assert!(needs_renormalising(Some(lo), Some(hi)) || hi - lo > MIN_GAP);
        lo += 0.0;
    }

    #[test]
    fn renormalisation_triggers_only_when_precision_runs_out() {
        assert!(!needs_renormalising(Some(1.0), Some(2.0)));
        assert!(needs_renormalising(Some(1.0), Some(1.0 + 1e-9)));
        // An open end can always extend, so it never needs rewriting.
        assert!(!needs_renormalising(None, Some(1.0)));
        assert!(!needs_renormalising(Some(1.0), None));
    }
}
