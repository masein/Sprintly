//! Sprintly API entry point.
//!
//! Subcommands:
//!   sprintly-api              — boot the HTTP server (default)
//!   sprintly-api migrate      — run SQLx migrations, exit 0 on success
//!   sprintly-api healthcheck  — used by Docker HEALTHCHECK; hits /readyz on
//!                                the configured bind address
//!
//! The point of bundling these into one binary is that the runtime image only
//! needs to ship one thing.

use std::process::ExitCode;

use sprintly_api::{app, config::Config, infra};
use tracing::{error, info};

#[tokio::main]
async fn main() -> ExitCode {
    // Load .env in dev. In prod the env is set by the orchestrator.
    let _ = dotenvy::dotenv();

    // Subcommand dispatch is intentionally trivial — no clap dependency just
    // to parse three keywords.
    let arg = std::env::args().nth(1);
    match arg.as_deref() {
        Some("migrate") => run(cmd_migrate()).await,
        Some("healthcheck") => run(cmd_healthcheck()).await,
        Some("--help") | Some("-h") => {
            println!("usage: sprintly-api [migrate|healthcheck]");
            ExitCode::SUCCESS
        }
        _ => run(cmd_serve()).await,
    }
}

async fn run<F: std::future::Future<Output = anyhow::Result<()>>>(f: F) -> ExitCode {
    match f.await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(error = %e, "fatal");
            ExitCode::from(1)
        }
    }
}

async fn cmd_serve() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;
    sprintly_api::logging::init(&cfg);

    info!(
        env = %cfg.env,
        bind = %cfg.api_bind,
        "sprintly-api booting"
    );

    let state = infra::AppState::connect(&cfg).await?;
    // Background worker: scans for achievement-eligibility every few minutes
    // plus anything else we drop into the `jobs` table later.
    sprintly_api::jobs::spawn(state.db.clone());
    let router = app::router(state.clone());

    let listener = tokio::net::TcpListener::bind(&cfg.api_bind).await?;
    info!(addr = %cfg.api_bind, "listening");

    // `into_make_service_with_connect_info` so handlers can extract the
    // remote SocketAddr (vault audit log uses it). In production this is the
    // upstream proxy address — X-Forwarded-For parsing lands in M10.
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    info!("shutdown complete");
    Ok(())
}

async fn cmd_migrate() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;
    sprintly_api::logging::init(&cfg);
    info!("running migrations");
    let pool = infra::db::connect(&cfg).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    info!("migrations applied");
    Ok(())
}

async fn cmd_healthcheck() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;
    // Hit our own /readyz over loopback. No TLS, no auth needed.
    let url = format!("http://{}/api/v1/readyz", cfg.api_bind);
    // Use a tiny one-shot via hyper — avoids pulling reqwest into runtime.
    let response = tokio::process::Command::new("wget")
        .args(["-qO-", "--tries=1", "--timeout=2", &url])
        .status()
        .await?;
    if response.success() {
        Ok(())
    } else {
        anyhow::bail!("readyz check failed")
    }
}

async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    tokio::select! {
        _ = term.recv() => info!("SIGTERM received"),
        _ = int.recv() => info!("SIGINT received"),
    }
}
