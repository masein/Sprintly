//! `sprintly-seed` — idempotent demo data loader.
//!
//! M1: ensures the demo admin account exists.
//!   email:    demo@sprintly.local
//!   password: sprintly
//!   handle:   demo
//!   role:     admin
//!
//! Later milestones extend this with projects, boards, tasks, sprints, time
//! logs, and a closed retro so the acceptance criteria in spec §12 work.
//!
//! Safe to run multiple times — every write is "INSERT … ON CONFLICT DO
//! NOTHING" or guarded by a SELECT.

use anyhow::Result;
use sprintly_api::{config::Config, domain::password, infra, logging};
use tracing::info;
use uuid::Uuid;

const DEMO_EMAIL: &str = "demo@sprintly.local";
const DEMO_HANDLE: &str = "demo";
const DEMO_DISPLAY: &str = "Sprintly Demo";
const DEMO_PASSWORD: &str = "sprintly";

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let cfg = Config::from_env()?;
    logging::init(&cfg);

    info!("seed: connecting");
    let db = infra::db::connect(&cfg).await?;

    let existing: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE email = $1 AND deleted_at IS NULL")
            .bind(DEMO_EMAIL)
            .fetch_optional(&db)
            .await?;

    match existing {
        Some(id) => {
            info!(%id, "seed: demo user already present, skipping");
        }
        None => {
            let hash = password::hash(&cfg.auth, DEMO_PASSWORD)?;
            let id = Uuid::now_v7();
            sqlx::query(
                r#"
                INSERT INTO users (id, email, handle, display_name, password_hash, role)
                VALUES ($1, $2, $3, $4, $5, 'admin')
                "#,
            )
            .bind(id)
            .bind(DEMO_EMAIL)
            .bind(DEMO_HANDLE)
            .bind(DEMO_DISPLAY)
            .bind(&hash)
            .execute(&db)
            .await?;
            info!(%id, email = DEMO_EMAIL, "seed: created demo admin");
        }
    }

    // TODO(sprintly): M2 seeds a "Sprintly Internal" project + members. M3
    // seeds tasks. M5 seeds an active + a closed sprint with a retro. M7
    // seeds a vault item. Layered in as each milestone lands.

    println!("done.");
    println!("login: {DEMO_EMAIL} / {DEMO_PASSWORD}");
    Ok(())
}
