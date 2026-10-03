//! A browser's sign-in, once accounts are on (see [`crate::users`]).
//!
//! The cookie carries 32 random bytes, hex; the table keeps only their
//! SHA-256, so reading the table signs nobody in. A session slides: each use
//! pushes its expiry out to [`TTL_DAYS`], written at most once a minute so a
//! busy dashboard is not a write per request. A password change, a reset or
//! a disable deletes every session of that account.

use crate::db::Db;
use crate::users::User;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const TTL_DAYS: i32 = 30;

fn digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Start a session; the returned token goes in the cookie and nowhere else.
pub async fn create(db: &Db, user: Uuid) -> anyhow::Result<String> {
    let token = hex::encode(rand::random::<[u8; 32]>());
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at)
         VALUES ($1, $2, now() + make_interval(days => $3))",
    )
    .bind(digest(&token))
    .bind(user)
    .bind(TTL_DAYS)
    .execute(&db.pool)
    .await?;
    Ok(token)
}

/// The account a token signs in, if it is live and the account is enabled.
pub async fn lookup(db: &Db, token: &str) -> anyhow::Result<Option<User>> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let hash = digest(token);
    let row: Option<Uuid> = sqlx::query_scalar(
        "UPDATE sessions
            SET last_seen_at = now(), expires_at = now() + make_interval(days => $2)
          WHERE token_hash = $1 AND expires_at > now()
            AND last_seen_at < now() - interval '1 minute'
         RETURNING user_id",
    )
    .bind(&hash)
    .bind(TTL_DAYS)
    .fetch_optional(&db.pool)
    .await?;
    let user = match row {
        Some(user) => Some(user),
        None => {
            sqlx::query_scalar(
                "SELECT user_id FROM sessions WHERE token_hash = $1 AND expires_at > now()",
            )
            .bind(&hash)
            .fetch_optional(&db.pool)
            .await?
        }
    };
    let Some(user) = user else { return Ok(None) };
    Ok(crate::users::get(db, user).await?.filter(|u| !u.disabled))
}

pub async fn revoke(db: &Db, token: &str) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(digest(token))
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// Drop expired sessions. Called from the scheduler's hourly prune.
pub async fn prune(db: &Db) -> anyhow::Result<u64> {
    Ok(
        sqlx::query("DELETE FROM sessions WHERE expires_at <= now()")
            .execute(&db.pool)
            .await?
            .rows_affected(),
    )
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::testdb;

    #[tokio::test]
    async fn a_session_signs_in_until_revoked() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let db = &t.db;
        let admin = crate::users::create_admin(db, "admin", "a long password")
            .await
            .unwrap();
        let token = create(db, admin.id).await.unwrap();
        assert_eq!(
            lookup(db, &token).await.unwrap().map(|u| u.id),
            Some(admin.id)
        );
        assert!(lookup(db, "not a token").await.unwrap().is_none());
        assert!(lookup(db, &"0".repeat(64)).await.unwrap().is_none());

        // Only the digest is stored.
        let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_ne!(stored, token.as_bytes());

        revoke(db, &token).await.unwrap();
        assert!(lookup(db, &token).await.unwrap().is_none());
        t.finish().await;
    }

    #[tokio::test]
    async fn an_expired_session_is_refused_and_pruned() {
        let Some(t) = testdb::fresh().await else {
            return;
        };
        let db = &t.db;
        let admin = crate::users::create_admin(db, "admin", "a long password")
            .await
            .unwrap();
        let token = create(db, admin.id).await.unwrap();
        sqlx::query("UPDATE sessions SET expires_at = now() - interval '1 second'")
            .execute(&db.pool)
            .await
            .unwrap();
        assert!(lookup(db, &token).await.unwrap().is_none());
        assert_eq!(prune(db).await.unwrap(), 1);
        t.finish().await;
    }
}
