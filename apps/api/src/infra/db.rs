//! Postgres connection pool. Tuning kept boring; reach for pgbouncer later if
//! we need it.

use anyhow::Result;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::str::FromStr;
use std::time::Duration;

use crate::config::Config;

pub async fn connect(cfg: &Config) -> Result<PgPool> {
    let opts = PgConnectOptions::from_str(&cfg.database_url)?.application_name("sprintly-api");

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(300)))
        .test_before_acquire(true)
        .connect_with(opts)
        .await?;

    Ok(pool)
}
