//! What a person has let an app do.
//!
//! Kept apart from what the manifest *requests* on purpose. A rebuild that
//! starts asking for a new scope has to surface as a question, and it can only
//! do that if the asking and the allowing are two separate records.

use super::scope::Scope;
use crate::Db;
use sqlx::Row;
use uuid::Uuid;

/// A grant, with enough about it to be worth revoking.
#[derive(Debug, Clone)]
pub struct Grant {
    pub scope: Scope,
    pub granted_at: chrono::DateTime<chrono::Utc>,
    /// `None` reads as "granted, never used" — the sentence that makes a
    /// permissions screen worth opening, and one that cannot be worked out
    /// afterwards if it was not recorded as it happened.
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list(db: &Db, app_id: Uuid) -> anyhow::Result<Vec<Grant>> {
    let rows = sqlx::query(
        "SELECT scope, granted_at, last_used_at FROM app_grants
          WHERE app_id = $1 ORDER BY scope",
    )
    .bind(app_id)
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            // A scope this build no longer knows about is skipped rather than
            // guessed at: a row left behind by an older version must not widen
            // into whatever it now happens to sort next to.
            Scope::parse(r.get::<String, _>("scope").as_str()).map(|scope| Grant {
                scope,
                granted_at: r.get("granted_at"),
                last_used_at: r.get("last_used_at"),
            })
        })
        .collect())
}

/// Just the scopes, for a permission check.
pub async fn of(db: &Db, app_id: Uuid) -> anyhow::Result<Vec<Scope>> {
    Ok(list(db, app_id)
        .await?
        .into_iter()
        .map(|g| g.scope)
        .collect())
}

/// Replace an app's grants with exactly this set.
///
/// The whole set rather than one at a time, because the screen shows all of
/// them at once and sending a diff would mean the UI and the server having to
/// agree about what was there before.
pub async fn set(db: &Db, app_id: Uuid, scopes: &[Scope]) -> anyhow::Result<()> {
    let wanted: Vec<String> = scopes.iter().map(|s| s.as_str().to_string()).collect();
    let mut tx = db.pool.begin().await?;
    sqlx::query("DELETE FROM app_grants WHERE app_id = $1 AND NOT (scope = ANY($2))")
        .bind(app_id)
        .bind(&wanted)
        .execute(&mut *tx)
        .await?;
    for scope in &wanted {
        // Existing rows keep their granted_at and last_used_at: re-saving a
        // screen must not make every grant look new.
        sqlx::query(
            "INSERT INTO app_grants (app_id, scope) VALUES ($1, $2)
             ON CONFLICT (app_id, scope) DO NOTHING",
        )
        .bind(app_id)
        .bind(scope)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Note that a grant was used. Best effort: failing to record it must never
/// fail the thing it was recording.
pub async fn touch(db: &Db, app_id: Uuid, scope: Scope) -> anyhow::Result<()> {
    sqlx::query("UPDATE app_grants SET last_used_at = now() WHERE app_id = $1 AND scope = $2")
        .bind(app_id)
        .bind(scope.as_str())
        .execute(&db.pool)
        .await?;
    Ok(())
}
