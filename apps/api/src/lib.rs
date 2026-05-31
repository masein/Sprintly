//! Sprintly API library crate.
//!
//! The binary in `main.rs` is intentionally thin — everything testable lives
//! here. Module map:
//!
//!   app        — Axum router composition
//!   config     — typed env config
//!   error      — single `AppError` enum + IntoResponse
//!   logging    — tracing subscriber setup
//!   infra      — DB / Redis / object storage clients and shared state
//!   routes     — HTTP handlers, grouped by resource
//!   middleware — request ID, auth extraction, rate limit
//!   domain     — pure business logic (auth, permissions, vault crypto)
//!   jobs       — background workers (achievements, cleanup, …)

pub mod app;
pub mod config;
pub mod domain;
pub mod error;
pub mod infra;
pub mod jobs;
pub mod logging;
pub mod middleware;
pub mod routes;

pub use error::AppError;
pub type AppResult<T> = std::result::Result<T, AppError>;
