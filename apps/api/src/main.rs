//! Sprintly API entry point.
//!
//! Subcommands:
//!   sprintly-api              — boot the HTTP server (default)
//!   sprintly-api migrate      — run SQLx migrations, exit 0 on success
//!   sprintly-api healthcheck  — used by Docker HEALTHCHECK; hits /readyz on
//!                                the configured bind address
//!   sprintly-api check-config — validate env + print a redacted summary
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
        Some("check-config") => run(cmd_check_config()).await,
        Some("restore") => run(cmd_restore(std::env::args().skip(2).collect())).await,
        Some("--help") | Some("-h") => {
            println!(
                "usage: sprintly-api [migrate|healthcheck|check-config|restore <backup-id> --confirm]"
            );
            ExitCode::SUCCESS
        }
        _ => run(cmd_serve()).await,
    }
}

async fn run<F: std::future::Future<Output = anyhow::Result<()>>>(f: F) -> ExitCode {
    match f.await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Print to stderr unconditionally: config errors happen before the
            // tracing subscriber is initialised, so `error!` alone is silent.
            // `{e:#}` includes the anyhow cause chain (which names the bad var).
            eprintln!("sprintly-api: fatal: {e:#}");
            error!(error = %e, "fatal");
            ExitCode::from(1)
        }
    }
}

async fn cmd_check_config() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;
    println!("config OK:\n{}", cfg.redacted_summary());
    Ok(())
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
    // Background worker: achievement scans, backups, and webhook delivery. The
    // vault master key lets it decrypt webhook signing secrets.
    sprintly_api::jobs::spawn(state.db.clone(), cfg.vault.master_key);
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
    // Migrations only need the database — not the full app config (JWT/vault/
    // MinIO secrets). Keeps the compose `migrate` one-shot env minimal.
    sprintly_api::logging::init_basic();
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("missing required env var: DATABASE_URL"))?;
    info!("running migrations");
    let pool = infra::db::connect_url(&database_url).await?;
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

/// Restore a backup into the target database (F15). This is intentionally a
/// CLI one-shot, never an API button: it OVERWRITES data, so it demands an
/// explicit `--confirm` and is audit-logged.
///
///   sprintly-api restore <backup-id> --confirm
///
/// Target DB is `SPRINTLY_RESTORE_DATABASE_URL` if set (point this at staging
/// for a drill), else `DATABASE_URL`.
async fn cmd_restore(args: Vec<String>) -> anyhow::Result<()> {
    sprintly_api::logging::init_basic();

    let backup_id = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or_else(|| anyhow::anyhow!("usage: sprintly-api restore <backup-id> --confirm"))?;
    let backup_id = uuid::Uuid::parse_str(backup_id)
        .map_err(|_| anyhow::anyhow!("backup-id must be a UUID"))?;
    let confirmed = args.iter().any(|a| a == "--confirm");

    let cfg = Config::from_env()?;
    let pool = infra::db::connect_url(&cfg.database_url).await?;

    let storage_key = sprintly_api::domain::backups::storage_key_of(&pool, backup_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no completed backup with id {backup_id}"))?;

    let target = std::env::var("SPRINTLY_RESTORE_DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cfg.database_url.clone());

    if !confirmed {
        eprintln!(
            "About to restore backup {backup_id}\n  object: {storage_key}\n  into:   {}\n\n\
             This OVERWRITES the target database. Re-run with --confirm to proceed.",
            mask_db_url(&target)
        );
        anyhow::bail!("refusing to restore without --confirm");
    }

    // Download the dump from MinIO. Point the presigner at the internal
    // endpoint (we're inside the cluster), like the worker does.
    let mut minio = cfg.minio.clone();
    minio.public_endpoint = minio.endpoint.clone();
    let signer = infra::s3::Presigner::new(&minio);
    let url = signer.get(&storage_key, None, 900);
    let tmp = format!("/tmp/sprintly-restore-{backup_id}.dump");
    info!(%backup_id, "downloading backup object");
    let dl = tokio::process::Command::new("curl")
        .args(["--fail-with-body", "-sS", "-o", &tmp, &url])
        .status()
        .await?;
    if !dl.success() {
        anyhow::bail!("failed to download backup object from MinIO");
    }

    info!("running pg_restore (this overwrites the target database)");
    let restore = tokio::process::Command::new("pg_restore")
        .args([
            "--clean",
            "--if-exists",
            "--no-owner",
            "--no-acl",
            "--dbname",
            &target,
            &tmp,
        ])
        .status()
        .await;
    let _ = tokio::fs::remove_file(&tmp).await;
    let restore = restore?;

    // Audit the attempt regardless of pg_restore's exit (a partial restore is
    // worth recording). actor is NULL — this is an out-of-band operator action.
    sqlx::query(
        r#"INSERT INTO admin_audit_log (id, actor_id, action, payload, user_agent)
           VALUES ($1, NULL, 'backup.restore', $2, 'sprintly-api restore (cli)')"#,
    )
    .bind(uuid::Uuid::now_v7())
    .bind(serde_json::json!({
        "backup_id": backup_id,
        "storage_key": storage_key,
        "ok": restore.success(),
    }))
    .execute(&pool)
    .await?;

    if !restore.success() {
        anyhow::bail!("pg_restore exited {restore}");
    }
    info!("restore complete");
    println!("restore complete: backup {backup_id} → target database");
    Ok(())
}

fn mask_db_url(url: &str) -> String {
    // Hide credentials in the printed confirmation line.
    match (url.find("://"), url.rfind('@')) {
        (Some(s), Some(a)) if a > s + 3 => format!("{}://***@{}", &url[..s], &url[a + 1..]),
        _ => url.to_string(),
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
