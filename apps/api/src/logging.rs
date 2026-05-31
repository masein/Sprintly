//! Tracing setup. JSON in prod, pretty in dev. `RUST_LOG` controls the filter.

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::Config;

pub fn init(cfg: &Config) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sprintly_api=debug,sqlx=warn,tower_http=info"));

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
