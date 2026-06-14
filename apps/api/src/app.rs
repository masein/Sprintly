//! Router composition. Each resource lives in `routes::*` and exports its own
//! sub-router; this file just bolts them together with middleware.

use std::time::Duration;

use axum::{middleware as axum_mw, routing::get, Router};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::{infra::AppState, middleware as own_mw, routes};

// `TimeoutLayer::new` is deprecated in this tower-http in favour of
// `with_status_code`, but `new` (which responds 408) is exactly the behaviour
// we want and the replacement's signature is version-sensitive; allow it here.
#[allow(deprecated)]
pub fn router(state: AppState) -> Router {
    // Liveness/readiness live outside /api/v1 so dumb HTTP checks work.
    let probes = Router::new()
        .route("/healthz", get(routes::health::liveness))
        .route("/readyz", get(routes::health::readiness));

    let v1 = Router::new()
        .merge(probes.clone())
        .merge(routes::auth::router())
        .merge(routes::users::router())
        .merge(routes::two_factor::router())
        .merge(routes::api_tokens::router())
        .merge(routes::admin::router())
        .merge(routes::projects::router())
        .merge(routes::boards::router())
        .merge(routes::board_views::router())
        .merge(routes::roadmap::router())
        .merge(routes::templates::router())
        .merge(routes::tasks::router())
        .merge(routes::task_detail::router())
        .merge(routes::search::router())
        .merge(routes::time_logs::router())
        .merge(routes::timesheets::router())
        .merge(routes::sprints::router())
        .merge(routes::retros::router())
        .merge(routes::dashboards::router())
        .merge(routes::metrics::router())
        .merge(routes::labels::router())
        .merge(routes::fields::router())
        .merge(routes::vault::router())
        .merge(routes::payroll::router())
        .merge(routes::invoicing::router())
        .merge(routes::notifications::router())
        .merge(routes::integrations::router())
        .merge(routes::achievements::router())
        .merge(routes::admin_panel::router())
        .merge(routes::backups::router())
        .merge(routes::webhooks::router())
        .merge(routes::ws::router());

    Router::new()
        .nest("/api/v1", v1)
        // Bare /healthz too, for orchestrators that prefer root probes.
        .merge(probes)
        .layer(axum_mw::from_fn(own_mw::csrf::csrf_guard))
        .with_state(state)
        .layer(
            tower::ServiceBuilder::new()
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(TraceLayer::new_for_http())
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(CompressionLayer::new())
                .layer(TimeoutLayer::new(Duration::from_secs(30)))
                .layer(permissive_cors_for_dev()),
        )
}

/// CORS in dev is permissive so the Next dev server (port 3000) can hit the
/// API directly. In prod, everything is same-origin via Caddy, so CORS is
/// effectively unused.
fn permissive_cors_for_dev() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
}
