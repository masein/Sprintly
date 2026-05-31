//! Shared infrastructure clients: DB, Redis, object storage.
//!
//! `AppState` holds them and is `.clone()`-cheap because the underlying
//! handles are all `Arc`-shaped.

pub mod db;
pub mod events;
pub mod pdf;
pub mod redis_pool;
pub mod s3;

use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: PgPool,
    pub redis: deadpool_redis::Pool,
}

impl AppState {
    pub async fn connect(cfg: &Config) -> Result<Self> {
        let db = db::connect(cfg).await?;
        let redis = redis_pool::connect(cfg)?;
        Ok(Self {
            cfg: Arc::new(cfg.clone()),
            db,
            redis,
        })
    }
}
