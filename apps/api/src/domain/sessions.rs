//! Session + refresh-token lifecycle.
//!
//! A session is one login chain. Many refresh tokens belong to it across
//! time, each one rotating to the next. The current valid token is the leaf
//! of the chain — `rotated_to IS NULL AND revoked_at IS NULL AND
//! expires_at > now()`.
//!
//! Reuse detection: if a client presents a token whose `rotated_to` is set,
//! that means someone already used it and rotated. The most likely cause is
//! token theft (the attacker rotated, the victim then presented the stolen
//! original). We revoke the entire session — every refresh token in the
//! chain stops working immediately.
//!
//! We do NOT enforce reuse detection by looking at `revoked_at` alone — a
//! token may be revoked because the session was cleanly logged out, which
//! shouldn't escalate to "session-family compromise".

use chrono::{Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    config::AuthConfig,
    domain::tokens::{self, RefreshSecret},
    AppError, AppResult,
};

#[derive(Debug)]
pub struct IssuedSession {
    pub session_id: Uuid,
    pub refresh: RefreshSecret,
}

/// Create a new session + first refresh token. Called by login and by
/// register-then-immediately-log-in.
pub async fn create(
    pool: &PgPool,
    cfg: &AuthConfig,
    user_id: Uuid,
    user_agent: Option<&str>,
    ip: Option<std::net::IpAddr>,
) -> AppResult<IssuedSession> {
    let session_id = Uuid::now_v7();
    let token_id = Uuid::now_v7();
    let refresh = tokens::mint_refresh();
    let expires_at = Utc::now() + Duration::seconds(cfg.refresh_ttl_secs as i64);

    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO sessions (id, user_id, user_agent, ip)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(session_id)
    .bind(user_id)
    .bind(user_agent)
    .bind(ip.map(sqlx::types::ipnetwork::IpNetwork::from))
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO refresh_tokens (id, session_id, user_id, token_hash, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(token_id)
    .bind(session_id)
    .bind(user_id)
    .bind(refresh.hash.as_slice())
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(IssuedSession {
        session_id,
        refresh,
    })
}

/// Outcome of attempting to use a refresh token.
pub enum RotateOutcome {
    /// Token was valid and current. Returns the new token to set in the cookie
    /// and the session id (unchanged across rotations).
    Rotated {
        session_id: Uuid,
        user_id: Uuid,
        role: String,
        refresh: RefreshSecret,
    },
}

/// Rotate a refresh token. On reuse-of-stale-token, revoke the whole family
/// and return `Unauthorized`.
pub async fn rotate(pool: &PgPool, cfg: &AuthConfig, plaintext: &str) -> AppResult<RotateOutcome> {
    let presented = tokens::hash_refresh(plaintext)?;

    let mut tx = pool.begin().await?;

    // Look up the presented token. We need its session, user, and rotation
    // state. Note: we don't filter by `revoked_at` here — we want to see
    // revoked-but-rotated tokens to distinguish reuse from "this session is
    // dead".
    let row = sqlx::query!(
        r#"
        SELECT  rt.id            AS "id!: Uuid",
                rt.session_id    AS "session_id!: Uuid",
                rt.user_id       AS "user_id!: Uuid",
                rt.expires_at    AS "expires_at!: chrono::DateTime<chrono::Utc>",
                rt.rotated_to,
                rt.revoked_at,
                s.revoked_at     AS session_revoked_at,
                u.role           AS "role!: String",
                u.status         AS "status!: String"
        FROM    refresh_tokens rt
        JOIN    sessions s ON s.id = rt.session_id
        JOIN    users    u ON u.id = rt.user_id
        WHERE   rt.token_hash = $1
        "#,
        presented.as_slice()
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };

    // ── reuse detection ────────────────────────────────────────────────────
    // If this token has already been rotated, we're seeing a stale token. We
    // burn the whole session family and tell the caller to log in again.
    if row.rotated_to.is_some() {
        sqlx::query(
            r#"
            UPDATE sessions
               SET revoked_at = now(),
                   revoked_reason = 'reuse_detected'
             WHERE id = $1
            "#,
        )
        .bind(row.session_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE refresh_tokens
               SET revoked_at = now()
             WHERE session_id = $1
               AND revoked_at IS NULL
            "#,
        )
        .bind(row.session_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        return Err(AppError::Unauthorized);
    }

    // Token is current. Other rejections:
    if row.revoked_at.is_some() || row.session_revoked_at.is_some() {
        return Err(AppError::Unauthorized);
    }
    if row.expires_at <= Utc::now() {
        return Err(AppError::Unauthorized);
    }
    if row.status != "active" {
        return Err(AppError::Unauthorized);
    }

    // ── mint successor ─────────────────────────────────────────────────────
    let new_id = Uuid::now_v7();
    let new_refresh = tokens::mint_refresh();
    let new_expires = Utc::now() + Duration::seconds(cfg.refresh_ttl_secs as i64);

    sqlx::query(
        r#"
        INSERT INTO refresh_tokens (id, session_id, user_id, token_hash, expires_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(new_id)
    .bind(row.session_id)
    .bind(row.user_id)
    .bind(new_refresh.hash.as_slice())
    .bind(new_expires)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        UPDATE refresh_tokens
           SET rotated_to = $1,
               rotated_at = now()
         WHERE id = $2
        "#,
    )
    .bind(new_id)
    .bind(row.id)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE sessions SET last_used_at = now() WHERE id = $1")
        .bind(row.session_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(RotateOutcome::Rotated {
        session_id: row.session_id,
        user_id: row.user_id,
        role: row.role,
        refresh: new_refresh,
    })
}

/// Revoke a session (logout). Idempotent.
pub async fn revoke(pool: &PgPool, session_id: Uuid, reason: &str) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        UPDATE sessions
           SET revoked_at = COALESCE(revoked_at, now()),
               revoked_reason = COALESCE(revoked_reason, $2)
         WHERE id = $1
        "#,
    )
    .bind(session_id)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"
        UPDATE refresh_tokens
           SET revoked_at = now()
         WHERE session_id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(session_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Look up the user_id+role tied to a still-live session. Used by the
/// middleware to enforce session revocation independently of JWT expiry.
pub async fn session_is_live(pool: &PgPool, session_id: Uuid) -> AppResult<bool> {
    let live: Option<bool> =
        sqlx::query_scalar(r#"SELECT revoked_at IS NULL FROM sessions WHERE id = $1"#)
            .bind(session_id)
            .fetch_optional(pool)
            .await?;
    Ok(live.unwrap_or(false))
}
