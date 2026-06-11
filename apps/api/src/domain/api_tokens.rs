//! Personal API tokens (F12) — scriptable, scoped access to the REST API.
//!
//! Wire format: `slt_` + base64url(no-padding) of 32 random bytes. The DB
//! stores sha256 of the raw bytes, so a leaked dump can't be replayed.
//! Revoke = DELETE, which takes effect on the next request. Scopes are
//! coarse: `read` covers safe methods, `write` covers everything.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::{AppError, AppResult};

pub const TOKEN_PREFIX: &str = "slt_";
pub const SCOPES: [&str; 2] = ["read", "write"];

/// What we hand back from list/create — never the hash, never the secret.
#[derive(Debug, Serialize, FromRow)]
pub struct ApiToken {
    pub id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// A freshly minted token: plaintext for the user (once), hash for the DB.
#[derive(Debug)]
pub struct MintedSecret {
    pub plaintext: String,
    pub hash: [u8; 32],
}

pub fn mint() -> MintedSecret {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    MintedSecret {
        plaintext: format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(raw)),
        hash: sha256(&raw),
    }
}

/// Hash a presented token. None for anything that isn't shaped like ours —
/// the caller falls through to the JWT path.
pub fn hash_presented(token: &str) -> Option<[u8; 32]> {
    let b64 = token.strip_prefix(TOKEN_PREFIX)?;
    let raw = URL_SAFE_NO_PAD.decode(b64).ok()?;
    if raw.len() != 32 {
        return None;
    }
    Some(sha256(&raw))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

pub fn valid_scopes(scopes: &[String]) -> bool {
    !scopes.is_empty()
        && scopes.iter().all(|s| SCOPES.contains(&s.as_str()))
        && scopes.len() <= SCOPES.len()
}

/// `write` implies `read`; a read-only token can't touch unsafe methods.
pub fn scope_allows(scopes: &[String], is_write: bool) -> bool {
    if is_write {
        scopes.iter().any(|s| s == "write")
    } else {
        scopes.iter().any(|s| s == "read" || s == "write")
    }
}

// ─── CRUD ───────────────────────────────────────────────────────────────────

pub async fn list(db: &PgPool, user_id: Uuid) -> AppResult<Vec<ApiToken>> {
    let rows = sqlx::query_as(
        r#"SELECT id, name, scopes, last_used_at, expires_at, created_at
           FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC"#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(rows)
}

/// Create a token; returns the row plus the plaintext (the only time it
/// exists outside the caller's hands).
pub async fn create(
    db: &PgPool,
    user_id: Uuid,
    name: &str,
    scopes: &[String],
    expires_at: Option<DateTime<Utc>>,
) -> AppResult<(ApiToken, String)> {
    let secret = mint();
    let row: ApiToken = sqlx::query_as(
        r#"INSERT INTO api_tokens (id, user_id, name, token_hash, scopes, expires_at)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, name, scopes, last_used_at, expires_at, created_at"#,
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(name)
    .bind(secret.hash.as_slice())
    .bind(scopes)
    .bind(expires_at)
    .fetch_one(db)
    .await?;
    Ok((row, secret.plaintext))
}

/// Revoke = delete. Immediate: the next lookup misses.
pub async fn revoke(db: &PgPool, user_id: Uuid, id: Uuid) -> AppResult<()> {
    let r = sqlx::query(r#"DELETE FROM api_tokens WHERE id = $1 AND user_id = $2"#)
        .bind(id)
        .bind(user_id)
        .execute(db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── authentication ─────────────────────────────────────────────────────────

/// The identity a valid token resolves to.
#[derive(Debug)]
pub struct TokenIdentity {
    pub user_id: Uuid,
    pub role: String,
}

/// Authenticate a presented token for a request. Checks shape, hash,
/// expiry, user status, and scope-vs-method; touches `last_used_at` on
/// success. Every failure is `Unauthorized` except an out-of-scope method,
/// which is `Forbidden` (the token is real — it just can't do that).
pub async fn authenticate(
    db: &PgPool,
    presented: &str,
    is_write: bool,
) -> AppResult<TokenIdentity> {
    let hash = hash_presented(presented).ok_or(AppError::Unauthorized)?;

    #[derive(FromRow)]
    struct Row {
        id: Uuid,
        user_id: Uuid,
        scopes: Vec<String>,
        expires_at: Option<DateTime<Utc>>,
        role: String,
        status: String,
    }
    let row: Row = sqlx::query_as(
        r#"
        SELECT t.id, t.user_id, t.scopes, t.expires_at, u.role, u.status
        FROM   api_tokens t
        JOIN   users u ON u.id = t.user_id AND u.deleted_at IS NULL
        WHERE  t.token_hash = $1
        "#,
    )
    .bind(hash.as_slice())
    .fetch_optional(db)
    .await?
    .ok_or(AppError::Unauthorized)?;

    if row.expires_at.is_some_and(|e| e <= Utc::now()) {
        return Err(AppError::Unauthorized);
    }
    if row.status != "active" {
        return Err(AppError::Unauthorized);
    }
    if !scope_allows(&row.scopes, is_write) {
        return Err(AppError::Forbidden);
    }

    sqlx::query(r#"UPDATE api_tokens SET last_used_at = now() WHERE id = $1"#)
        .bind(row.id)
        .execute(db)
        .await?;

    Ok(TokenIdentity {
        user_id: row.user_id,
        role: row.role,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_hash_round_trip() {
        let m = mint();
        assert!(m.plaintext.starts_with(TOKEN_PREFIX));
        assert_eq!(hash_presented(&m.plaintext).unwrap(), m.hash);
    }

    #[test]
    fn presented_garbage_is_not_ours() {
        assert!(hash_presented("a-jwt-looking-thing.abc.def").is_none());
        assert!(hash_presented("slt_not-base64!!").is_none());
        // Right prefix, wrong byte count.
        let short = format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode([0u8; 8]));
        assert!(hash_presented(&short).is_none());
    }

    #[test]
    fn scope_validation() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(valid_scopes(&s(&["read"])));
        assert!(valid_scopes(&s(&["read", "write"])));
        assert!(!valid_scopes(&s(&[])));
        assert!(!valid_scopes(&s(&["admin"])));
    }

    #[test]
    fn write_implies_read_but_not_vice_versa() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(scope_allows(&s(&["read"]), false));
        assert!(!scope_allows(&s(&["read"]), true));
        assert!(scope_allows(&s(&["write"]), false));
        assert!(scope_allows(&s(&["write"]), true));
    }
}
