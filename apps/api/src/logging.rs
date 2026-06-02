//! Tracing setup. JSON in prod, pretty in dev. `RUST_LOG` controls the filter.

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::Config;

fn default_filter() -> EnvFilter {
    EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sprintly_api=debug,sqlx=warn,tower_http=info"))
}

/// Minimal subscriber for subcommands that don't load the full `Config`
/// (e.g. `migrate`). Compact, `RUST_LOG`-driven.
pub fn init_basic() {
    tracing_subscriber::registry()
        .with(default_filter())
        .with(tracing_subscriber::fmt::layer().with_target(true).compact())
        .init();
}

pub fn init(cfg: &Config) {
    let filter = default_filter();

    let registry = tracing_subscriber::registry().with(filter);

    if cfg.is_dev() {
        registry
            .with(tracing_subscriber::fmt::layer().with_target(true).compact())
            .init();
    } else {
        registry
            .with(tracing_subscriber::fmt::layer().json().with_target(true))
            .init();
    }
}
