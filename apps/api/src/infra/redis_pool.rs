//! Redis pool. Used for: rate limiting, refresh-token reuse detection,
//! idempotency keys, and WebSocket pub/sub fan-out.

use anyhow::Result;
use deadpool_redis::{Config as DpConfig, Pool, Runtime};

use crate::config::Config;

pub fn connect(cfg: &Config) -> Result<Pool> {
    let dp = DpConfig::from_url(&cfg.redis_url);
    let pool = dp.create_pool(Some(Runtime::Tokio1))?;
    Ok(pool)
}
