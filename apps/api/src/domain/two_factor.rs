//! Persistence for two-factor auth (F11). The crypto lives in [`super::totp`];
//! this module is the thin DB layer over the `users` 2FA columns
//! (`totp_secret`, `totp_enrolled_at`, `backup_codes`).
//!
//! Lifecycle:
//!   • `enroll_pending` stores a fresh secret with `totp_enrolled_at = NULL`
//!     — the secret exists but logins are NOT yet gated.
//!   • `activate` flips `totp_enrolled_at` and writes hashed recovery codes,
//!     but only if a pending secret is present (so you can't enable without
//!     proving you can generate a code first).
//!   • `disable` wipes all three columns.
//!
//! The secret is stored base64url(no-pad) of the raw 20 bytes.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{domain::totp, AppResult};

/// Whether 2FA is set up and/or active for a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// A secret exists (enrollment started).
    pub has_secret: bool,
    /// Enrollment is confirmed — logins are gated on a second factor.
    pub enabled: bool,
}

/// Store a fresh (pending) secret, resetting any prior enrollment. Idempotent
/// from the caller's view: re-enrolling overwrites and clears recovery codes.
pub async fn enroll_pending(db: &PgPool, user_id: Uuid, secret: &[u8]) -> AppResult<()> {
    let encoded = URL_SAFE_NO_PAD.encode(secret);
    sqlx::query(
        r#"UPDATE users
              SET totp_secret = $2, totp_enrolled_at = NULL, backup_codes = '{}'
            WHERE id = $1"#,
    )
    .bind(user_id)
    .bind(encoded)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn status(db: &PgPool, user_id: Uuid) -> AppResult<Status> {
    let row: Option<(bool, bool)> = sqlx::query_as(
        r#"SELECT totp_secret IS NOT NULL, totp_enrolled_at IS NOT NULL
             FROM users WHERE id = $1"#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    let (has_secret, enabled) = row.unwrap_or((false, false));
    Ok(Status {
        has_secret,
        enabled,
    })
}

/// The decoded secret regardless of activation state — used by `activate` to
/// verify the confirming code against the pending secret.
pub async fn pending_secret(db: &PgPool, user_id: Uuid) -> AppResult<Option<Vec<u8>>> {
    let enc: Option<String> = sqlx::query_scalar(r#"SELECT totp_secret FROM users WHERE id = $1"#)
        .bind(user_id)
        .fetch_optional(db)
        .await?
        .flatten();
    Ok(enc.and_then(|e| URL_SAFE_NO_PAD.decode(e).ok()))
}

/// The decoded secret ONLY if 2FA is active — used at login step-up. Returns
/// `None` for users who never enrolled or only started enrollment.
pub async fn secret_if_enabled(db: &PgPool, user_id: Uuid) -> AppResult<Option<Vec<u8>>> {
    let enc: Option<String> = sqlx::query_scalar(
        r#"SELECT totp_secret FROM users
            WHERE id = $1 AND totp_enrolled_at IS NOT NULL"#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?
    .flatten();
    Ok(enc.and_then(|e| URL_SAFE_NO_PAD.decode(e).ok()))
}

/// Confirm enrollment: set `totp_enrolled_at` and store hashed recovery codes.
/// Only succeeds when a pending (not-yet-active) secret exists. Returns `true`
/// if it flipped, `false` if there was nothing pending to activate.
pub async fn activate(db: &PgPool, user_id: Uuid, hashed_codes: &[String]) -> AppResult<bool> {
    let res = sqlx::query(
        r#"UPDATE users
              SET totp_enrolled_at = now(), backup_codes = $2
            WHERE id = $1
              AND totp_secret IS NOT NULL
              AND totp_enrolled_at IS NULL"#,
    )
    .bind(user_id)
    .bind(hashed_codes)
    .execute(db)
    .await?;
    Ok(res.rows_affected() == 1)
}

/// Turn 2FA off entirely.
pub async fn disable(db: &PgPool, user_id: Uuid) -> AppResult<()> {
    sqlx::query(
        r#"UPDATE users
              SET totp_secret = NULL, totp_enrolled_at = NULL, backup_codes = '{}'
            WHERE id = $1"#,
    )
    .bind(user_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Atomically spend a single recovery code. Hashes the input, removes it from
/// the array iff present, and reports whether a code was actually consumed —
/// so a code can never be used twice (the `array_remove` is the consume).
pub async fn consume_recovery_code(db: &PgPool, user_id: Uuid, code: &str) -> AppResult<bool> {
    let hash = totp::hash_recovery_code(code);
    let res = sqlx::query(
        r#"UPDATE users
              SET backup_codes = array_remove(backup_codes, $2)
            WHERE id = $1 AND $2 = ANY(backup_codes)"#,
    )
    .bind(user_id)
    .bind(&hash)
    .execute(db)
    .await?;
    Ok(res.rows_affected() == 1)
}
