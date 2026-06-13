//! Redis fixed-window rate limiting.
//!
//! A counter per `key` with a TTL equal to the window. The first hit sets the
//! TTL; subsequent hits within the window increment it. Once the count passes
//! `limit` we reject with `AppError::RateLimited { retry_after }`, where
//! `retry_after` is the remaining TTL so clients know when to come back.
//!
//! Limits are env-tunable (see `*_per_*` helpers) with sensible defaults. The
//! same primitive backs vault reveals and the auth endpoints.

use crate::{infra::AppState, AppError, AppResult};

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Login attempts allowed per minute, per client IP.
pub fn login_ip_per_min() -> u32 {
    env_u32("SPRINTLY_RL_LOGIN_IP_PER_MIN", 20)
}
/// Login attempts allowed per minute, per email.
pub fn login_email_per_min() -> u32 {
    env_u32("SPRINTLY_RL_LOGIN_EMAIL_PER_MIN", 8)
}
/// Password-reset requests allowed per hour, per client IP.
pub fn reset_ip_per_hour() -> u32 {
    env_u32("SPRINTLY_RL_RESET_IP_PER_HOUR", 10)
}
/// Password-reset requests allowed per hour, per email.
pub fn reset_email_per_hour() -> u32 {
    env_u32("SPRINTLY_RL_RESET_EMAIL_PER_HOUR", 5)
}
/// Second-factor (TOTP / recovery code) attempts allowed per minute, per user.
pub fn twofa_per_min() -> u32 {
    env_u32("SPRINTLY_RL_2FA_PER_MIN", 10)
}

/// Core fixed-window check against any Redis connection. Testable without the
/// full `AppState` (see `tests/rate_limit.rs`).
pub async fn hit_conn<C>(conn: &mut C, key: &str, limit: u32, window_secs: u64) -> AppResult<()>
where
    C: redis::aio::ConnectionLike + Send,
{
    let n: i64 = redis::cmd("INCR").arg(key).query_async(&mut *conn).await?;
    if n == 1 {
        // First hit in the window — arm the TTL so the bucket drains.
        let _: () = redis::cmd("EXPIRE")
            .arg(key)
            .arg(window_secs)
            .query_async(&mut *conn)
            .await?;
    }
    if n as u32 > limit {
        let ttl: i64 = redis::cmd("TTL")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .unwrap_or(window_secs as i64);
        return Err(AppError::RateLimited {
            retry_after: ttl.max(1) as u64,
        });
    }
    Ok(())
}

/// Convenience wrapper that pulls a connection from the app's Redis pool.
pub async fn hit(state: &AppState, key: &str, limit: u32, window_secs: u64) -> AppResult<()> {
    let mut conn = state
        .redis
        .get()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("redis: {e}")))?;
    hit_conn(&mut conn, key, limit, window_secs).await
}
